import type { Camera, Rect } from "../camera";
import { drawSprite, layersReady, tileHash, type GameAssets, type Sprite, type Variations } from "./assets";
import type { TileCache } from "./tileCache";

const CHUNK = 32;
// Textured chunks: 16 or 32 texture pixels per tile, chosen from the on-screen scale.
const HIGH_RES = 32;
const LOW_RES = 16;
const HIGH_RES_MIN_SCALE = 20;
const BYTE_BUDGET = 256 * 1024 * 1024;
const BUILD_BUDGET_MS = 4;

interface Chunk {
  canvas: HTMLCanvasElement;
  bytes: number;
}

// Textured terrain as lazily built 32x32-tile canvases, LRU-evicted by byte budget.
// Chunks are aligned to Factorio's chunk grid (source coordinates). A chunk is only built
// once all the tile images it needs have loaded; until then the caller's color
// cache shows through.
export class TerrainCache {
  private chunks = new Map<number, Chunk>();
  private bytes = 0;
  private revision = -1;
  private res = 0;
  private tileSprites = new Map<number, Variations | undefined>();
  private pending: { key: number; cx: number; cy: number; dist: number }[] = [];
  private batch: (Sprite | null)[] = new Array(CHUNK * CHUNK).fill(null);
  // Total chunks built, for measurements.
  builtCount = 0;

  constructor(
    private assets: GameAssets,
    private tiles: TileCache,
  ) {}

  get residentChunks(): number {
    return this.chunks.size;
  }

  draw(ctx: CanvasRenderingContext2D, camera: Camera, view: Rect, dpr: number): void {
    const { tiles } = this;
    if (tiles.revision !== this.revision) {
      this.revision = tiles.revision;
      this.tileSprites.clear();
      this.clear();
    }
    const [ox, oy] = tiles.info.source_offset;
    const cx0 = Math.floor((view.x0 + ox) / CHUNK);
    const cx1 = Math.floor((view.x1 + ox) / CHUNK);
    const cy0 = Math.floor((view.y0 + oy) / CHUNK);
    const cy1 = Math.floor((view.y1 + oy) / CHUNK);

    const scale = camera.zoom * dpr;
    const visibleCount = (cx1 - cx0 + 1) * (cy1 - cy0 + 1);
    const wantHigh = scale >= HIGH_RES_MIN_SCALE && visibleCount * chunkBytes(HIGH_RES) <= BYTE_BUDGET;
    const res = wantHigh ? HIGH_RES : LOW_RES;
    if (res !== this.res) {
      this.res = res;
      this.clear();
    }

    this.pending.length = 0;
    const pending = this.pending;
    const { x_min, y_min, width, height } = tiles.info;
    for (let cy = cy0; cy <= cy1; cy++) {
      for (let cx = cx0; cx <= cx1; cx++) {
        const tx0 = cx * CHUNK - ox;
        const ty0 = cy * CHUNK - oy;
        if (tx0 + CHUNK <= x_min || ty0 + CHUNK <= y_min || tx0 >= x_min + width || ty0 >= y_min + height) continue;
        const key = chunkKey(cx, cy);
        const chunk = this.chunks.get(key);
        if (chunk) {
          // Re-insert to mark as most recently used.
          this.chunks.delete(key);
          this.chunks.set(key, chunk);
        } else {
          const dx = tx0 + CHUNK / 2 - camera.x;
          const dy = ty0 + CHUNK / 2 - camera.y;
          pending.push({ key, cx, cy, dist: dx * dx + dy * dy });
        }
      }
    }

    if (pending.length) {
      pending.sort((a, b) => a.dist - b.dist);
      const start = performance.now();
      for (const p of pending) {
        const chunk = this.build(p.cx, p.cy);
        if (chunk) {
          this.chunks.set(p.key, chunk);
          this.bytes += chunk.bytes;
          this.builtCount++;
        }
        if (performance.now() - start > BUILD_BUDGET_MS) break;
      }
      this.evict(cx0, cy0, cx1, cy1);
    }

    // Draw in device pixels with rounded edges so neighbouring chunks never leave seams.
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "medium";
    const sx = (wx: number) => Math.round((camera.viewW / 2 + (wx - camera.x) * camera.zoom) * dpr);
    const sy = (wy: number) => Math.round((camera.viewH / 2 + (wy - camera.y) * camera.zoom) * dpr);
    for (let cy = cy0; cy <= cy1; cy++) {
      for (let cx = cx0; cx <= cx1; cx++) {
        const chunk = this.chunks.get(chunkKey(cx, cy));
        if (!chunk) continue;
        const tx0 = cx * CHUNK - ox;
        const ty0 = cy * CHUNK - oy;
        const dx0 = sx(tx0);
        const dy0 = sy(ty0);
        ctx.drawImage(chunk.canvas, dx0, dy0, sx(tx0 + CHUNK) - dx0, sy(ty0 + CHUNK) - dy0);
      }
    }
    ctx.restore();
  }

  clear(): void {
    this.chunks.clear();
    this.bytes = 0;
  }

  // Oldest-used first. Chunks in the current view were touched this frame, so they go last.
  private evict(cx0: number, cy0: number, cx1: number, cy1: number): void {
    for (const [key, chunk] of this.chunks) {
      if (this.bytes <= BYTE_BUDGET) break;
      const cx = (key % 65536) - 32768;
      const cy = Math.floor(key / 65536) - 32768;
      if (cx >= cx0 && cx <= cx1 && cy >= cy0 && cy <= cy1) continue;
      this.chunks.delete(key);
      this.bytes -= chunk.bytes;
    }
  }

  private variations(id: number): Variations | undefined {
    if (!this.tileSprites.has(id)) {
      const name = this.tiles.palette[id]?.name;
      this.tileSprites.set(id, name ? this.assets.tile(name) : undefined);
    }
    return this.tileSprites.get(id);
  }

  // Returns null while any needed image is still loading.
  private build(cx: number, cy: number): Chunk | null {
    const { tiles, res } = this;
    const { x_min, y_min, width, height, source_offset } = tiles.info;
    const [ox, oy] = source_offset;
    const tx0 = cx * CHUNK - ox;
    const ty0 = cy * CHUNK - oy;
    const batch = this.batch;

    let blocked = false;
    for (let j = 0; j < CHUNK; j++) {
      const ty = ty0 + j;
      for (let i = 0; i < CHUNK; i++) {
        const tx = tx0 + i;
        let sprite: Sprite | null = null;
        if (tx >= x_min && tx < x_min + width && ty >= y_min && ty < y_min + height) {
          const id = tiles.tileIds[(ty - y_min) * width + (tx - x_min)];
          const variations = this.variations(id);
          if (variations) {
            // Hash source coordinates so a recentered export keeps its look.
            const v = variations[tileHash(tx + ox, ty + oy) % variations.length];
            sprite = v[0] ?? null;
            if (sprite && !sprite.slot.failed && !layersReady(v)) blocked = true;
          }
        }
        batch[j * CHUNK + i] = sprite;
      }
    }
    if (blocked) return null;

    const size = CHUNK * res;
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext("2d")!;
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    // drawSprite works in tile units.
    ctx.scale(res, res);
    for (let j = 0; j < CHUNK; j++) {
      for (let i = 0; i < CHUNK; i++) {
        const sprite = batch[j * CHUNK + i];
        if (sprite?.slot.ready) {
          drawSprite(ctx, sprite, i + 0.5, j + 0.5);
        } else {
          // Missing graphics for this tile id or a failed image: palette color.
          const tx = tx0 + i;
          const ty = ty0 + j;
          if (tx < x_min || tx >= x_min + width || ty < y_min || ty >= y_min + height) continue;
          const color = tiles.palette[tiles.tileIds[(ty - y_min) * width + (tx - x_min)]]?.color ?? [255, 0, 255];
          ctx.fillStyle = `rgb(${color[0]}, ${color[1]}, ${color[2]})`;
          ctx.fillRect(i, j, 1, 1);
        }
      }
    }
    return { canvas, bytes: size * size * 4 };
  }
}

const chunkBytes = (res: number) => CHUNK * res * CHUNK * res * 4;
const chunkKey = (cx: number, cy: number) => (cy + 32768) * 65536 + (cx + 32768);
