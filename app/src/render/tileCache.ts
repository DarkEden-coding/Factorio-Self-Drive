import type { MapInfo, ResourceInfo, SimApi, TileInfo } from "../sim/types";

const ORE_BLEND = 0.65;

// One pixel per tile, tile colors with the ore overlay blended in. Rebuilt when the
// sim's tile_revision changes; shared by the main view and the minimap. Also keeps the
// raw grids so the textured renderer does not have to copy them out of wasm each frame.
export class TileCache {
  readonly canvas = document.createElement("canvas");
  info: MapInfo = { name: "", width: 1, height: 1, x_min: 0, y_min: 0, seed: 0, source_offset: [0, 0] };
  palette: TileInfo[] = [];
  orePalette: ResourceInfo[] = [];
  tileIds: Uint8Array = new Uint8Array(0);
  ore: Uint8Array = new Uint8Array(0);
  revision = -1;

  invalidate(): void {
    this.revision = -1;
  }

  ensure(sim: SimApi): void {
    const revision = sim.tileRevision();
    if (revision === this.revision) return;
    this.revision = revision;
    this.rebuild(sim);
  }

  private rebuild(sim: SimApi): void {
    const info = sim.mapInfo();
    this.info = info;
    this.canvas.width = info.width;
    this.canvas.height = info.height;
    const palette = sim.tilePalette();
    const orePalette = sim.resourcePalette();
    const tiles = sim.tiles();
    const ore = sim.resources();
    this.palette = palette;
    this.orePalette = orePalette;
    this.tileIds = tiles;
    this.ore = ore;
    const ctx = this.canvas.getContext("2d")!;
    const image = ctx.createImageData(info.width, info.height);
    const px = image.data;
    for (let i = 0; i < tiles.length; i++) {
      let [r, g, b] = palette[tiles[i]]?.color ?? [255, 0, 255];
      const oreColor = ore[i] ? orePalette[ore[i]]?.color : undefined;
      if (oreColor) {
        r += (oreColor[0] - r) * ORE_BLEND;
        g += (oreColor[1] - g) * ORE_BLEND;
        b += (oreColor[2] - b) * ORE_BLEND;
      }
      px[i * 4] = r;
      px[i * 4 + 1] = g;
      px[i * 4 + 2] = b;
      px[i * 4 + 3] = 255;
    }
    ctx.putImageData(image, 0, 0);
  }
}
