import type { VehicleState } from "../sim/types";
import { drawSprite, layersReady, type GameAssets, type Sprite } from "./assets";

const PRELOAD_DIRECTIONS = 3;
const DESTROYED_TINT: [number, number, number] = [0.3, 0.28, 0.27];

export interface VehicleLayers {
  shadows: Sprite[];
  body: Sprite[];
}

// Picks the vehicle frame for an orientation (docs/assets.md: 64 pre-rotated directions,
// direction 0 north, no extra rotation). Loads the nearby frames ahead of a turn and keeps
// showing the last complete frame until the wanted one has loaded.
export class VehicleGraphics {
  private last: { kind: string; layers: VehicleLayers } | null = null;

  constructor(private assets: GameAssets) {}

  layers(v: VehicleState): VehicleLayers | null {
    const sprites = this.assets.vehicle(v.kind);
    if (!sprites) return null;
    const n = sprites.directionCount;
    const index = Math.floor((((v.orientation % 1) + 1) % 1) * n + 0.5) % n;
    const wanted = sprites.directions[index];
    if (layersReady(wanted)) {
      for (let d = 1; d <= PRELOAD_DIRECTIONS; d++) {
        layersReady(sprites.directions[(index + d) % n]);
        layersReady(sprites.directions[(index - d + n) % n]);
      }
      const layers = { shadows: wanted.filter((s) => s.shadow), body: wanted.filter((s) => !s.shadow) };
      this.last = { kind: v.kind, layers };
      return layers;
    }
    return this.last?.kind === v.kind ? this.last.layers : null;
  }
}

export function drawVehicleSprites(ctx: CanvasRenderingContext2D, layers: VehicleLayers, v: VehicleState, part: "shadows" | "body"): void {
  if (part === "shadows") {
    for (const s of layers.shadows) if (s.slot.ready) drawSprite(ctx, s, v.x, v.y);
    return;
  }
  // A destroyed vehicle stays where it stopped, darkened and sitting in a scorch mark.
  if (v.destroyed) {
    const [[ax, ay], [bx, by]] = v.collision_box;
    ctx.beginPath();
    ctx.ellipse(v.x, v.y, (bx - ax) * 0.85, (by - ay) * 0.6, v.orientation * Math.PI * 2, 0, Math.PI * 2);
    ctx.fillStyle = "rgba(10, 8, 6, 0.55)";
    ctx.fill();
  }
  for (const s of layers.body) if (s.slot.ready) drawSprite(ctx, s, v.x, v.y, v.destroyed ? DESTROYED_TINT : null);
}
