import type { Rect } from "../camera";
import type { MapEntity, SimApi } from "../sim/types";
import { drawSprite, layersReady, type GameAssets, type Sprite } from "./assets";

// Sprites reach up to ~10 tiles from their anchor (tall trees, spawners), so fetch generously.
const FETCH_MARGIN = 32;
const SHADOW_ALPHA = 0.5;

// Entities for the visible viewport. Re-queries the sim only when the view leaves the
// last fetched area or map_revision changes.
export class EntityCache {
  private entities: MapEntity[] = [];
  private covered: Rect | null = null;
  private revision = -1;
  // Bumped on every refetch, so derived data knows when to rebuild.
  version = 0;

  get(sim: SimApi, view: Rect): MapEntity[] {
    const revision = sim.mapRevision();
    const c = this.covered;
    const inside = c && view.x0 >= c.x0 && view.y0 >= c.y0 && view.x1 <= c.x1 && view.y1 <= c.y1;
    if (!inside || revision !== this.revision) {
      this.covered = {
        x0: view.x0 - FETCH_MARGIN,
        y0: view.y0 - FETCH_MARGIN,
        x1: view.x1 + FETCH_MARGIN,
        y1: view.y1 + FETCH_MARGIN,
      };
      this.revision = revision;
      this.version++;
      this.entities = sim.entitiesInRect(this.covered.x0, this.covered.y0, this.covered.x1, this.covered.y1);
    }
    return this.entities;
  }
}

// An entity with its sprites resolved. Paint order inside a layer list is kept.
export interface Drawable {
  e: MapEntity;
  shadows: Sprite[];
  layers: Sprite[];
  all: Sprite[];
  ready: boolean;
  // World-space bounds of every sprite, for culling.
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

// Sorted-by-y list of fetched entities. Ones without manifest graphics (unknown
// modded names) have no layers and are drawn as palette shapes.
export class SpriteScene {
  drawables: Drawable[] = [];
  private version = -1;
  private visible: Drawable[] = [];

  constructor(private assets: GameAssets) {}

  update(cache: EntityCache, entities: MapEntity[]): void {
    if (cache.version === this.version) return;
    this.version = cache.version;
    const list: Drawable[] = [];
    for (const e of entities) {
      const variations = e.kind === "cliff" && e.cliff_orientation ? this.assets.cliff(e.cliff_orientation) : this.assets.entity(e.name);
      // Cliff graphics already encode their orientation: never rotate by e.orientation.
      const layers = variations?.[(e.id - 1) % variations.length];
      list.push(makeDrawable(e, layers ?? []));
    }
    list.sort((a, b) => a.e.y - b.e.y || a.e.id - b.e.id);
    this.drawables = list;
  }

  // Shadow pass over everything visible, then non-shadow layers in y order.
  // `vehicle` is drawn at its y position in that order.
  draw(
    ctx: CanvasRenderingContext2D,
    view: Rect,
    zoom: number,
    vehicle: { y: number; drawShadows(): void; drawBody(): void } | null,
  ): void {
    const visible = this.visible;
    visible.length = 0;
    for (const d of this.drawables) {
      if (d.x1 < view.x0 || d.x0 > view.x1 || d.y1 < view.y0 || d.y0 > view.y1) continue;
      if (!d.ready && d.all.length) d.ready = layersReady(d.all);
      visible.push(d);
    }

    // Factorio composites its shadow layer translucently; raw shadow PNGs drawn opaque look black.
    ctx.globalAlpha = SHADOW_ALPHA;
    for (const d of visible) {
      if (!d.ready) continue;
      for (const s of d.shadows) if (s.slot.ready) drawSprite(ctx, s, d.e.x, d.e.y);
    }
    vehicle?.drawShadows();
    ctx.globalAlpha = 1;

    let vehiclePending = vehicle !== null;
    for (const d of visible) {
      if (vehiclePending && d.e.y > vehicle!.y) {
        vehicle!.drawBody();
        vehiclePending = false;
      }
      if (d.ready) for (const s of d.layers) drawSprite(ctx, s, d.e.x, d.e.y);
      else drawEntityShape(ctx, d.e, zoom);
    }
    if (vehiclePending) vehicle!.drawBody();
  }
}

function makeDrawable(e: MapEntity, sprites: Sprite[]): Drawable {
  let x0 = e.x;
  let y0 = e.y;
  let x1 = e.x;
  let y1 = e.y;
  for (const s of sprites) {
    x0 = Math.min(x0, e.x + s.shiftX - s.dw / 2);
    x1 = Math.max(x1, e.x + s.shiftX + s.dw / 2);
    y0 = Math.min(y0, e.y + s.shiftY - s.dh / 2);
    y1 = Math.max(y1, e.y + s.shiftY + s.dh / 2);
  }
  return {
    e,
    shadows: sprites.filter((s) => s.shadow),
    layers: sprites.filter((s) => !s.shadow),
    all: sprites,
    ready: false,
    x0,
    y0,
    x1,
    y1,
  };
}

// Debug palette view of every entity.
export function drawEntities(ctx: CanvasRenderingContext2D, entities: MapEntity[], zoom: number): void {
  for (const e of entities) drawEntityShape(ctx, e, zoom);
}

// Collision boxes are relative to (x, y) and rotate by `orientation` about their own center (docs/sim-api.md).
function drawEntityShape(ctx: CanvasRenderingContext2D, e: MapEntity, zoom: number): void {
  const px = 1 / zoom;
  const [[ax, ay], [bx, by]] = e.collision_box;
  const cx = e.x + (ax + bx) / 2;
  const cy = e.y + (ay + by) / 2;
  const w = bx - ax;
  const h = by - ay;

  ctx.beginPath();
  if (e.kind === "tree") {
    ctx.arc(cx, cy, Math.max(w, h) / 2, 0, Math.PI * 2);
    ctx.fillStyle = "#1d5a2c";
  } else if (e.kind === "rock") {
    ctx.ellipse(cx, cy, w / 2, h / 2, 0, 0, Math.PI * 2);
    ctx.fillStyle = "#8c8c8c";
  } else if (e.kind === "cliff") {
    rotatedRect(ctx, cx, cy, w, h, e.orientation);
    ctx.fillStyle = "#7b5330";
  } else {
    ctx.arc(cx, cy, Math.max(w, h) / 2, 0, Math.PI * 2);
    ctx.fillStyle = "#b32d2d";
  }
  ctx.fill();
  ctx.strokeStyle = "rgba(0, 0, 0, 0.45)";
  ctx.lineWidth = px;
  ctx.stroke();
}

// Collision box overlay, drawn on top of either view.
export function drawEntityOverlays(ctx: CanvasRenderingContext2D, entities: MapEntity[], view: Rect, zoom: number, showBoxes: boolean): void {
  if (!showBoxes) return;
  const px = 1 / zoom;
  ctx.lineWidth = px;
  for (const e of entities) {
    const [[ax, ay], [bx, by]] = e.collision_box;
    const cx = e.x + (ax + bx) / 2;
    const cy = e.y + (ay + by) / 2;
    if (cx < view.x0 - 2 || cx > view.x1 + 2 || cy < view.y0 - 2 || cy > view.y1 + 2) continue;
    ctx.beginPath();
    rotatedRect(ctx, cx, cy, bx - ax, by - ay, e.orientation);
    ctx.strokeStyle = "#ffe066";
    ctx.stroke();
  }
}

// Nest penalty zones: always part of the palette view, shown over game graphics with the box overlay.
export function drawNestZones(ctx: CanvasRenderingContext2D, entities: MapEntity[], zoom: number): void {
  for (const e of entities) {
    if (e.kind !== "nest") continue;
    const [[ax, ay], [bx, by]] = e.collision_box;
    ctx.beginPath();
    ctx.arc(e.x + (ax + bx) / 2, e.y + (ay + by) / 2, e.penalty_radius ?? 0, 0, Math.PI * 2);
    ctx.fillStyle = "rgba(220, 50, 50, 0.10)";
    ctx.fill();
    ctx.strokeStyle = "rgba(240, 70, 70, 0.7)";
    ctx.lineWidth = 1.5 / zoom;
    ctx.stroke();
  }
}

function rotatedRect(ctx: CanvasRenderingContext2D, cx: number, cy: number, w: number, h: number, orientation: number): void {
  const angle = orientation * Math.PI * 2;
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  const corners: [number, number][] = [[-w / 2, -h / 2], [w / 2, -h / 2], [w / 2, h / 2], [-w / 2, h / 2]];
  corners.forEach(([x, y], i) => {
    const px = cx + x * cos - y * sin;
    const py = cy + x * sin + y * cos;
    if (i === 0) ctx.moveTo(px, py);
    else ctx.lineTo(px, py);
  });
  ctx.closePath();
}
