import type { ObservationSpec, PolicySpec, SimApi } from "../sim/types";
import { el } from "./dom";

const UPDATE_INTERVAL_MS = 200;
const LOCAL_DISPLAY_PX = 88;

interface Channel {
  name: string;
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  image: ImageData;
}

interface Section {
  note: HTMLElement;
  channels: Channel[];
}

// Renders each observation channel as a grayscale image. While driving to a target it shows the
// u8 policy grids and vector features, exactly what the model sees. Otherwise it shows the
// app's f32 grids; values above 1 (raw friction) are scaled by the channel maximum.
export class ObsView {
  private local: Section;
  private coarse: Section;
  private vectorTitle = el("h4", { textContent: "Vector features" });
  private vectorRows: HTMLElement[];
  private vectorTable: HTMLElement;
  private lastUpdate = 0;

  constructor(
    root: HTMLElement,
    private sim: SimApi,
    private spec: ObservationSpec,
    policySpec: PolicySpec,
  ) {
    this.local = this.section(root, "Local", spec.local_size);
    this.coarse = this.section(root, "Coarse", spec.coarse_size);
    this.vectorRows = policySpec.vector_features.map(() => el("td", { className: "num" }));
    this.vectorTable = el(
      "table",
      { className: "vector-table" },
      policySpec.vector_features.map((name, i) => el("tr", {}, [el("td", { textContent: name }), this.vectorRows[i]])),
    );
    root.append(this.vectorTitle, this.vectorTable);
  }

  private section(root: HTMLElement, title: string, size: number): Section {
    const channels = this.spec.channels.map((name) => {
      const canvas = el("canvas", { width: size, height: size, className: "obs-canvas" });
      canvas.style.width = canvas.style.height = `${LOCAL_DISPLAY_PX}px`;
      const ctx = canvas.getContext("2d")!;
      return { name, canvas, ctx, image: ctx.createImageData(size, size) };
    });
    const note = el("small");
    root.append(
      el("h4", { textContent: `${title} observation` }, [note]),
      el(
        "div",
        { className: "obs-grid" },
        channels.map((c) => el("figure", {}, [c.canvas, el("figcaption", { textContent: c.name })])),
      ),
    );
    return { note, channels };
  }

  maybeUpdate(now: number, episodeActive: boolean): void {
    if (now - this.lastUpdate < UPDATE_INTERVAL_MS) return;
    this.lastUpdate = now;
    const { local_size: ls, coarse_size: cs, coarse_cell_tiles: ct } = this.spec;
    this.vectorTitle.hidden = this.vectorTable.hidden = !episodeActive;
    if (episodeActive) {
      this.local.note.textContent = `policy_local u8, ${ls}x${ls} tiles, snapped to the vehicle tile`;
      this.coarse.note.textContent = `policy_coarse u8, ${cs}x${cs} cells, ${ct} tiles/cell`;
      paint(this.local.channels, this.sim.policyLocal(), ls, 255);
      paint(this.coarse.channels, this.sim.policyCoarse(), cs, 255);
      const vector = this.sim.policyVector();
      vector.forEach((v, i) => (this.vectorRows[i].textContent = v.toFixed(3)));
    } else {
      this.local.note.textContent = `f32, ${ls}x${ls} tiles, 1 tile/px (set a target for the policy view)`;
      this.coarse.note.textContent = `f32, ${cs}x${cs} cells, ${ct} tiles/cell`;
      paint(this.local.channels, this.sim.observationLocal(), ls, null);
      paint(this.coarse.channels, this.sim.observationCoarse(), cs, null);
    }
  }
}

// `scale` null = per-channel maximum (at least 1).
function paint(channels: Channel[], data: ArrayLike<number>, size: number, scale: number | null): void {
  const cells = size * size;
  channels.forEach((ch, c) => {
    const base = c * cells;
    let max = scale ?? 1;
    if (scale === null) for (let i = 0; i < cells; i++) max = Math.max(max, data[base + i]);
    const px = ch.image.data;
    for (let i = 0; i < cells; i++) {
      const v = Math.round((Math.max(0, data[base + i]) / max) * 255);
      px[i * 4] = px[i * 4 + 1] = px[i * 4 + 2] = v;
      px[i * 4 + 3] = 255;
    }
    ch.ctx.putImageData(ch.image, 0, 0);
  });
}
