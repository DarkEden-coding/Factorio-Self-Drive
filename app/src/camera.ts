export interface Rect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

const MIN_ZOOM = 1;
const MAX_ZOOM = 80;

// World units are tiles, x east, y south. `zoom` is CSS pixels per tile.
export class Camera {
  x = 0;
  y = 0;
  zoom = 10;
  viewW = 1;
  viewH = 1;

  toWorld(sx: number, sy: number): { x: number; y: number } {
    return { x: this.x + (sx - this.viewW / 2) / this.zoom, y: this.y + (sy - this.viewH / 2) / this.zoom };
  }

  visibleRect(marginTiles = 0): Rect {
    const hw = this.viewW / 2 / this.zoom + marginTiles;
    const hh = this.viewH / 2 / this.zoom + marginTiles;
    return { x0: this.x - hw, y0: this.y - hh, x1: this.x + hw, y1: this.y + hh };
  }

  // Zooms keeping the world point under (sx, sy) fixed.
  zoomAt(sx: number, sy: number, factor: number): void {
    const before = this.toWorld(sx, sy);
    this.zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, this.zoom * factor));
    const after = this.toWorld(sx, sy);
    this.x += before.x - after.x;
    this.y += before.y - after.y;
  }

  panByPixels(dx: number, dy: number): void {
    this.x -= dx / this.zoom;
    this.y -= dy / this.zoom;
  }
}
