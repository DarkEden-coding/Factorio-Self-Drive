// Loader for the local Factorio graphics described in docs/assets.md.
// Everything under /game-assets/ is proprietary and gitignored; the app must keep working without it.

export const ASSETS_URL = "/game-assets/";
export const ASSETS_HINT = "Game graphics missing: run python3 scripts/build-game-assets.py";

interface RawSprite {
  image: string;
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
  shift: [number, number];
  draw_as_shadow: boolean;
  tint?: [number, number, number, number];
}

interface RawManifest {
  schema_version: number;
  tile_pixels: number;
  tiles: Record<string, { variations: RawSprite[][] }>;
  entities: Record<string, { variations: RawSprite[][] }>;
  cliffs: Record<string, { variations: RawSprite[][] }>;
  resources: Record<string, { variations: RawSprite[][] }>;
  vehicles: Record<string, { direction_count: number; directions: RawSprite[][] }>;
}

const IDLE = 0;
const LOADING = 1;
const READY = 2;
const FAILED = 3;

// One PNG, loaded the first time a sprite that uses it is wanted.
export class ImageSlot {
  state = IDLE;
  img: HTMLImageElement | null = null;

  constructor(readonly path: string) {}

  request(): void {
    if (this.state !== IDLE) return;
    this.state = LOADING;
    const img = new Image();
    img.decoding = "async";
    img.onload = () => {
      this.img = img;
      this.state = READY;
    };
    img.onerror = () => {
      this.state = FAILED;
      console.warn(`Failed to load ${ASSETS_URL}${this.path}`);
    };
    img.src = ASSETS_URL + this.path;
  }

  get ready(): boolean {
    return this.state === READY;
  }

  get failed(): boolean {
    return this.state === FAILED;
  }
}

// A manifest sprite with its destination size (tiles) precomputed.
export interface Sprite {
  slot: ImageSlot;
  sx: number;
  sy: number;
  sw: number;
  sh: number;
  // Destination size and center offset, in tiles.
  dw: number;
  dh: number;
  shiftX: number;
  shiftY: number;
  shadow: boolean;
  tint: [number, number, number] | null;
  alpha: number;
  tinted: Map<string, HTMLCanvasElement>;
}

export type Variations = Sprite[][];

export interface VehicleSprites {
  directionCount: number;
  directions: Sprite[][];
}

const isWhite = (t: number[]) => t[0] === 1 && t[1] === 1 && t[2] === 1;

export class GameAssets {
  private slots = new Map<string, ImageSlot>();
  private tiles = new Map<string, Variations>();
  private entities = new Map<string, Variations>();
  private cliffs = new Map<string, Variations>();
  private resources = new Map<string, Variations>();
  private vehicles = new Map<string, VehicleSprites>();
  readonly tilePixels: number;

  private constructor(raw: RawManifest) {
    this.tilePixels = raw.tile_pixels || 32;
    for (const [k, v] of Object.entries(raw.tiles)) this.tiles.set(k, this.convert(v.variations));
    for (const [k, v] of Object.entries(raw.entities)) this.entities.set(k, this.convert(v.variations));
    for (const [k, v] of Object.entries(raw.cliffs)) this.cliffs.set(k, this.convert(v.variations));
    for (const [k, v] of Object.entries(raw.resources)) this.resources.set(k, this.convert(v.variations));
    for (const [k, v] of Object.entries(raw.vehicles)) {
      this.vehicles.set(k, { directionCount: v.direction_count, directions: this.convert(v.directions) });
    }
  }

  // Resolves to null when there is no usable manifest.
  static async load(): Promise<GameAssets | null> {
    try {
      const res = await fetch(`${ASSETS_URL}manifest.json`);
      if (!res.ok || !(res.headers.get("content-type") ?? "").includes("json")) return null;
      const raw = (await res.json()) as RawManifest;
      return raw.schema_version === 1 ? new GameAssets(raw) : null;
    } catch {
      return null;
    }
  }

  tile(name: string): Variations | undefined {
    return this.tiles.get(name);
  }

  entity(name: string): Variations | undefined {
    return this.entities.get(name);
  }

  cliff(orientation: string): Variations | undefined {
    return this.cliffs.get(orientation);
  }

  resource(name: string): Variations | undefined {
    return this.resources.get(name);
  }

  vehicle(kind: string): VehicleSprites | undefined {
    return this.vehicles.get(kind);
  }

  private convert(variations: RawSprite[][]): Variations {
    return variations.map((layers) => layers.map((s) => this.sprite(s)));
  }

  private sprite(s: RawSprite): Sprite {
    let slot = this.slots.get(s.image);
    if (!slot) {
      slot = new ImageSlot(s.image);
      this.slots.set(s.image, slot);
    }
    const k = s.scale / this.tilePixels;
    const tint = s.tint && !isWhite(s.tint) ? ([s.tint[0], s.tint[1], s.tint[2]] as [number, number, number]) : null;
    return {
      slot,
      sx: s.x,
      sy: s.y,
      sw: s.width,
      sh: s.height,
      dw: s.width * k,
      dh: s.height * k,
      shiftX: s.shift[0],
      shiftY: s.shift[1],
      shadow: s.draw_as_shadow,
      tint,
      alpha: s.tint ? s.tint[3] : 1,
      tinted: new Map(),
    };
  }
}

// True when every non-shadow layer can be drawn; requests the missing images.
export function layersReady(layers: Sprite[]): boolean {
  let ok = true;
  for (const s of layers) {
    if (s.slot.ready) continue;
    s.slot.request();
    if (!s.shadow) ok = false;
  }
  return ok;
}

// Source pixels with the tint multiplied in (RGBA channels multiply; this is not an overlay).
// `extra` darkens or recolors further, used for the destroyed vehicle.
function tintedSource(s: Sprite, extra: [number, number, number] | null): HTMLCanvasElement {
  const key = extra ? extra.join(",") : "";
  let canvas = s.tinted.get(key);
  if (canvas) return canvas;
  canvas = document.createElement("canvas");
  canvas.width = s.sw;
  canvas.height = s.sh;
  const c = canvas.getContext("2d")!;
  const img = s.slot.img!;
  c.drawImage(img, s.sx, s.sy, s.sw, s.sh, 0, 0, s.sw, s.sh);
  const t = s.tint ?? [1, 1, 1];
  const m = extra ?? [1, 1, 1];
  c.globalCompositeOperation = "multiply";
  c.fillStyle = `rgb(${Math.round(t[0] * m[0] * 255)}, ${Math.round(t[1] * m[1] * 255)}, ${Math.round(t[2] * m[2] * 255)})`;
  c.fillRect(0, 0, s.sw, s.sh);
  // Multiply paints over transparent pixels too, so restore the original alpha.
  c.globalCompositeOperation = "destination-in";
  c.drawImage(img, s.sx, s.sy, s.sw, s.sh, 0, 0, s.sw, s.sh);
  s.tinted.set(key, canvas);
  return canvas;
}

// Draws a ready sprite centered at world (x, y) + shift. The context must be in tile units.
export function drawSprite(
  ctx: CanvasRenderingContext2D,
  s: Sprite,
  x: number,
  y: number,
  extra: [number, number, number] | null = null,
): void {
  const dx = x + s.shiftX - s.dw / 2;
  const dy = y + s.shiftY - s.dh / 2;
  const alpha = s.alpha !== 1;
  if (alpha) ctx.globalAlpha = s.alpha;
  if (s.tint || extra) ctx.drawImage(tintedSource(s, extra), 0, 0, s.sw, s.sh, dx, dy, s.dw, s.dh);
  else ctx.drawImage(s.slot.img!, s.sx, s.sy, s.sw, s.sh, dx, dy, s.dw, s.dh);
  if (alpha) ctx.globalAlpha = 1;
}

// Unsigned 32-bit hash of source tile coordinates (docs/assets.md).
export const tileHash = (x: number, y: number): number =>
  (Math.imul(x | 0, 73856093) ^ Math.imul(y | 0, 19349663)) >>> 0;
