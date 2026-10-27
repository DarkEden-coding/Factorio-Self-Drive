import type { Rect } from "../camera";
import { drawSprite, tileHash, type GameAssets, type Variations } from "./assets";
import type { TileCache } from "./tileCache";

// Ore sprites for every resource tile in view, each anchored at its tile center.
export class ResourceLayer {
  private byId = new Map<number, Variations | undefined>();
  private revision = -1;

  constructor(private assets: GameAssets) {}

  draw(ctx: CanvasRenderingContext2D, tiles: TileCache, view: Rect): void {
    if (tiles.revision !== this.revision) {
      this.revision = tiles.revision;
      this.byId.clear();
    }
    const { x_min, y_min, width, height, source_offset } = tiles.info;
    const [ox, oy] = source_offset;
    const tx0 = Math.max(x_min, Math.floor(view.x0) - 1);
    const tx1 = Math.min(x_min + width - 1, Math.ceil(view.x1) + 1);
    const ty0 = Math.max(y_min, Math.floor(view.y0) - 1);
    const ty1 = Math.min(y_min + height - 1, Math.ceil(view.y1) + 1);
    const { ore } = tiles;

    for (let ty = ty0; ty <= ty1; ty++) {
      let i = (ty - y_min) * width + (tx0 - x_min);
      for (let tx = tx0; tx <= tx1; tx++, i++) {
        const id = ore[i];
        if (!id) continue;
        let variations = this.byId.get(id);
        if (variations === undefined && !this.byId.has(id)) {
          const name = tiles.orePalette[id]?.name;
          variations = name ? this.assets.resource(name) : undefined;
          this.byId.set(id, variations);
        }
        if (!variations) continue;
        const layers = variations[tileHash(tx + ox, ty + oy) % variations.length];
        for (const s of layers) {
          if (s.slot.ready) drawSprite(ctx, s, tx + 0.5, ty + 0.5);
          else s.slot.request();
        }
      }
    }
  }
}
