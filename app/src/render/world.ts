import type { Camera, Rect } from "../camera";
import type { Settings } from "../settings";
import type { ObservationSpec, SimApi, TargetState, VehicleState } from "../sim/types";
import type { GameAssets } from "./assets";
import { drawEntities, drawEntityOverlays, drawNestZones, EntityCache, SpriteScene } from "./entities";
import { ResourceLayer } from "./resources";
import { TerrainCache } from "./terrain";
import type { TileCache } from "./tileCache";
import { drawVehicleSprites, VehicleGraphics } from "./vehicle";

const VEHICLE_COLORS: Record<string, string> = { car: "#4aa3df", tank: "#9aab5e" };

// Below these zooms (CSS pixels per tile) the cheap views take over, like Factorio's map view:
// map-color tiles under TEXTURE_MIN_ZOOM, palette shapes under SPRITE_MIN_ZOOM.
export const TEXTURE_MIN_ZOOM = 4;
export const SPRITE_MIN_ZOOM = 3;

interface GameLayers {
  terrain: TerrainCache;
  resources: ResourceLayer;
  scene: SpriteScene;
  vehicle: VehicleGraphics;
}

export class WorldRenderer {
  private ctx: CanvasRenderingContext2D;
  private entityCache = new EntityCache();
  private game: GameLayers | null = null;
  dpr = 1;

  constructor(
    private canvas: HTMLCanvasElement,
    private sim: SimApi,
    private tiles: TileCache,
    private settings: Settings,
    assets: GameAssets | null,
  ) {
    this.ctx = canvas.getContext("2d")!;
    if (assets) {
      this.game = {
        terrain: new TerrainCache(assets, tiles),
        resources: new ResourceLayer(assets),
        scene: new SpriteScene(assets),
        vehicle: new VehicleGraphics(assets),
      };
    }
  }

  resize(camera: Camera): void {
    this.dpr = window.devicePixelRatio || 1;
    camera.viewW = this.canvas.clientWidth;
    camera.viewH = this.canvas.clientHeight;
    this.canvas.width = Math.round(camera.viewW * this.dpr);
    this.canvas.height = Math.round(camera.viewH * this.dpr);
  }

  // `overlays` draws in world coordinates on top of the map, under the target and observation frames.
  // `snapObs` draws the observation frames snapped to the vehicle tile, like the policy grids.
  draw(
    camera: Camera,
    vehicle: VehicleState,
    target: TargetState | null,
    spec: ObservationSpec,
    overlays: (ctx: CanvasRenderingContext2D, zoom: number) => void,
    snapObs: boolean,
  ): void {
    const { ctx, dpr } = this;
    const z = camera.zoom;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = "#101214";
    ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
    ctx.setTransform(dpr * z, 0, 0, dpr * z, dpr * (camera.viewW / 2 - camera.x * z), dpr * (camera.viewH / 2 - camera.y * z));

    const info = this.tiles.info;
    ctx.imageSmoothingEnabled = false;
    // Always drawn: it is the whole view in palette mode, and the fallback under textured chunks still loading.
    ctx.drawImage(this.tiles.canvas, info.x_min, info.y_min, info.width, info.height);

    const view = camera.visibleRect();
    const entities = this.entityCache.get(this.sim, view);
    const game = this.settings.gameGraphics ? this.game : null;

    if (game && z >= TEXTURE_MIN_ZOOM) {
      game.terrain.draw(ctx, camera, view, dpr);
      game.resources.draw(ctx, this.tiles, view);
    }
    if (game && z >= SPRITE_MIN_ZOOM) {
      game.scene.update(this.entityCache, entities);
      const layers = game.vehicle.layers(vehicle);
      game.scene.draw(
        ctx,
        view,
        z,
        layers && {
          y: vehicle.y,
          drawShadows: () => drawVehicleSprites(ctx, layers, vehicle, "shadows"),
          drawBody: () => drawVehicleSprites(ctx, layers, vehicle, "body"),
        },
      );
      if (!layers) this.drawVehicle(vehicle, z);
    } else {
      drawNestZones(ctx, entities, z);
      drawEntities(ctx, entities, z);
      this.drawVehicle(vehicle, z);
    }

    if (this.settings.showGrid) this.drawGrid(view, z);
    if (this.settings.showBoxes) {
      if (game && z >= SPRITE_MIN_ZOOM) drawNestZones(ctx, entities, z);
      drawEntityOverlays(ctx, entities, view, z, true);
      this.drawVehicleBox(vehicle, z);
    }
    overlays(ctx, z);
    if (target) this.drawTarget(vehicle, target, z);
    if (this.settings.showObs) this.drawObservationAreas(vehicle, spec, z, snapObs);
  }

  private drawGrid(view: Rect, zoom: number): void {
    const { ctx } = this;
    const step = zoom >= 12 ? 1 : zoom >= 3 ? 8 : 32;
    ctx.lineWidth = 1 / zoom;
    for (const axis of ["x", "y"] as const) {
      const lo = axis === "x" ? view.x0 : view.y0;
      const hi = axis === "x" ? view.x1 : view.y1;
      for (let v = Math.floor(lo / step) * step; v <= hi; v += step) {
        ctx.strokeStyle = v % 32 === 0 ? "rgba(255, 255, 255, 0.28)" : "rgba(255, 255, 255, 0.11)";
        ctx.beginPath();
        if (axis === "x") {
          ctx.moveTo(v, view.y0);
          ctx.lineTo(v, view.y1);
        } else {
          ctx.moveTo(view.x0, v);
          ctx.lineTo(view.x1, v);
        }
        ctx.stroke();
      }
    }
  }

  private drawTarget(vehicle: VehicleState, target: TargetState, zoom: number): void {
    const { ctx } = this;
    const color = target.reached ? "#5ee08a" : "#ffd23f";
    ctx.setLineDash([6 / zoom, 5 / zoom]);
    ctx.strokeStyle = color;
    ctx.globalAlpha = 0.7;
    ctx.lineWidth = 1.5 / zoom;
    ctx.beginPath();
    ctx.moveTo(vehicle.x, vehicle.y);
    ctx.lineTo(target.x, target.y);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;

    ctx.beginPath();
    ctx.arc(target.x, target.y, target.radius, 0, Math.PI * 2);
    ctx.fillStyle = "rgba(255, 210, 63, 0.12)";
    ctx.fill();
    ctx.lineWidth = 2 / zoom;
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(target.x, target.y, 3 / zoom, 0, Math.PI * 2);
    ctx.fillStyle = color;
    ctx.fill();
  }

  // Both observation grids are axis-aligned and centered on the vehicle, or on the
  // corner at its tile + (0, 0) when snapped (local cell 32 is the vehicle tile).
  private drawObservationAreas(vehicle: VehicleState, spec: ObservationSpec, zoom: number, snap: boolean): void {
    const { ctx } = this;
    const cx = snap ? Math.floor(vehicle.x) : vehicle.x;
    const cy = snap ? Math.floor(vehicle.y) : vehicle.y;
    const local = spec.local_size;
    const coarse = spec.coarse_size * spec.coarse_cell_tiles;
    ctx.lineWidth = 1.5 / zoom;
    ctx.strokeStyle = "#35d0ff";
    ctx.strokeRect(cx - local / 2, cy - local / 2, local, local);
    ctx.setLineDash([8 / zoom, 6 / zoom]);
    ctx.strokeStyle = "#e07bff";
    ctx.strokeRect(cx - coarse / 2, cy - coarse / 2, coarse, coarse);
    ctx.setLineDash([]);
  }

  private drawVehicle(v: VehicleState, zoom: number): void {
    const { ctx } = this;
    const [[ax, ay], [bx, by]] = v.collision_box;
    ctx.save();
    ctx.translate(v.x, v.y);
    ctx.rotate(v.orientation * Math.PI * 2);

    ctx.fillStyle = v.destroyed ? "#3a3a3a" : (VEHICLE_COLORS[v.kind] ?? "#cccccc");
    ctx.fillRect(ax, ay, bx - ax, by - ay);
    ctx.lineWidth = 1.5 / zoom;
    ctx.strokeStyle = v.destroyed ? "#d33" : "#0b1a24";
    ctx.strokeRect(ax, ay, bx - ax, by - ay);

    // Heading marker: arrow at the front (north edge at orientation 0).
    const w = bx - ax;
    ctx.beginPath();
    ctx.moveTo(0, ay - 0.35);
    ctx.lineTo(w * 0.32, ay + 0.35);
    ctx.lineTo(-w * 0.32, ay + 0.35);
    ctx.closePath();
    ctx.fillStyle = v.destroyed ? "#777" : "#fff";
    ctx.fill();

    if (v.kind === "tank") {
      ctx.beginPath();
      ctx.arc(0, 0, w * 0.3, 0, Math.PI * 2);
      ctx.fillStyle = "rgba(0, 0, 0, 0.35)";
      ctx.fill();
    }
    ctx.restore();
  }

  private drawVehicleBox(v: VehicleState, zoom: number): void {
    const { ctx } = this;
    const [[ax, ay], [bx, by]] = v.collision_box;
    ctx.save();
    ctx.translate(v.x, v.y);
    ctx.rotate(v.orientation * Math.PI * 2);
    ctx.strokeStyle = "#ffe066";
    ctx.lineWidth = 1 / zoom;
    ctx.strokeRect(ax, ay, bx - ax, by - ay);
    ctx.restore();
  }
}
