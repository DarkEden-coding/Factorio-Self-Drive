import type { Camera } from "../camera";
import type { TargetState, VehicleState } from "../sim/types";
import type { TileCache } from "./tileCache";

export class Minimap {
  private ctx: CanvasRenderingContext2D;

  constructor(
    private canvas: HTMLCanvasElement,
    private tiles: TileCache,
    onJump: (x: number, y: number) => void,
  ) {
    this.ctx = canvas.getContext("2d")!;
    let dragging = false;
    const jump = (e: PointerEvent) => {
      const { scale, ox, oy } = this.layout();
      const info = this.tiles.info;
      onJump(info.x_min + (e.offsetX - ox) / scale, info.y_min + (e.offsetY - oy) / scale);
    };
    canvas.addEventListener("pointerdown", (e) => {
      dragging = true;
      canvas.setPointerCapture(e.pointerId);
      jump(e);
    });
    canvas.addEventListener("pointermove", (e) => dragging && jump(e));
    canvas.addEventListener("pointerup", () => (dragging = false));
  }

  // Map is fitted into the canvas, centered.
  private layout() {
    const { width, height } = this.tiles.info;
    const scale = Math.min(this.canvas.width / width, this.canvas.height / height);
    return { scale, ox: (this.canvas.width - width * scale) / 2, oy: (this.canvas.height - height * scale) / 2 };
  }

  draw(camera: Camera, vehicle: VehicleState, target: TargetState | null): void {
    const { ctx } = this;
    const info = this.tiles.info;
    const { scale, ox, oy } = this.layout();
    const toX = (x: number) => ox + (x - info.x_min) * scale;
    const toY = (y: number) => oy + (y - info.y_min) * scale;

    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(this.tiles.canvas, ox, oy, info.width * scale, info.height * scale);

    const view = camera.visibleRect();
    ctx.strokeStyle = "#fff";
    ctx.lineWidth = 1;
    ctx.strokeRect(toX(view.x0), toY(view.y0), (view.x1 - view.x0) * scale, (view.y1 - view.y0) * scale);

    if (target) {
      ctx.fillStyle = "#ffd23f";
      ctx.fillRect(toX(target.x) - 2, toY(target.y) - 2, 4, 4);
    }
    ctx.beginPath();
    ctx.arc(toX(vehicle.x), toY(vehicle.y), 3.5, 0, Math.PI * 2);
    ctx.fillStyle = vehicle.destroyed ? "#d33" : "#ff5a36";
    ctx.fill();
    ctx.strokeStyle = "#000";
    ctx.stroke();
  }
}
