import type { DecisionResult, EpisodeInfo, EpisodeRules, EpisodeState, FieldInfo, RetargetResult, SimApi } from "../sim/types";
import type { PlanOverlay, PolicyOutput } from "./types";

export interface FieldImage {
  info: FieldInfo;
  canvas: HTMLCanvasElement;
}

// The current RL episode in the sim, plus what the overlays and panels show about it.
// Every decision goes through `decide` / `followPath` so the trail stays in step.
export class EpisodeSession {
  info: EpisodeInfo | null = null;
  state: EpisodeState | null = null;
  last: DecisionResult | null = null;
  policy: PolicyOutput | null = null;
  plan: PlanOverlay | null = null;
  // Vehicle x,y at the start and after each decision.
  trail: number[] = [];
  // Increments on every retarget and clear, so drivers drop state from an earlier episode.
  generation = 0;
  private field: FieldImage | null = null;
  private baseline: { decisions: number; path: Float32Array } | null = null;

  constructor(private sim: SimApi) {}

  get running(): boolean {
    return this.state?.status === "running";
  }

  get decisions(): number {
    return this.state?.decisions ?? 0;
  }

  // Starts an episode toward (x, y) from the vehicle's current state. An unreachable
  // target leaves the current episode as it was.
  retarget(x: number, y: number, rules?: Partial<EpisodeRules>): RetargetResult {
    const result = this.sim.retarget(x, y, rules);
    if (result.reachable) this.begin(result);
    return result;
  }

  decide(action: number): DecisionResult {
    return this.record(this.sim.decide(action));
  }

  followPath(waypoints: Float32Array): DecisionResult {
    return this.record(this.sim.followPath(waypoints));
  }

  // Called once per frame. `place_random`, `set_vehicle` and `load_map` end the episode in the sim.
  sync(): void {
    if (!this.info) return;
    this.state = this.sim.episodeState();
    if (!this.state) this.clear();
  }

  clear(): void {
    this.info = null;
    this.state = null;
    this.last = null;
    this.policy = null;
    this.plan = null;
    this.trail = [];
    this.field = null;
    this.baseline = null;
    this.generation++;
  }

  // Distance field as an image, one pixel per tile, built once per episode.
  fieldImage(): FieldImage | null {
    if (!this.info) return null;
    if (!this.field) this.field = buildFieldImage(this.sim.distanceField(), this.sim.distanceFieldValues());
    return this.field;
  }

  // A* path from the vehicle, recomputed once per decision.
  baselinePath(): Float32Array | null {
    if (!this.info) return null;
    if (this.baseline?.decisions !== this.decisions) {
      this.baseline = { decisions: this.decisions, path: this.sim.baselinePath() };
    }
    return this.baseline.path;
  }

  private begin(info: EpisodeInfo): void {
    this.clear();
    this.info = info;
    this.state = this.sim.episodeState();
    this.trail = [info.start.x, info.start.y];
  }

  private record(result: DecisionResult): DecisionResult {
    const v = this.sim.vehicleState();
    this.trail.push(v.x, v.y);
    this.last = result;
    this.state = this.sim.episodeState();
    return result;
  }
}

// Blue (near the target) to red (far). Blocked and unreachable tiles stay transparent.
function buildFieldImage(info: FieldInfo, values: Float32Array): FieldImage {
  const canvas = document.createElement("canvas");
  canvas.width = info.width;
  canvas.height = info.height;
  let max = 0;
  for (const v of values) if (Number.isFinite(v) && v > max) max = v;
  const ctx = canvas.getContext("2d")!;
  const image = ctx.createImageData(info.width, info.height);
  for (let i = 0; i < values.length; i++) {
    const v = values[i];
    if (!Number.isFinite(v)) continue;
    const t = max > 0 ? v / max : 0;
    image.data[i * 4] = Math.round(40 + 215 * t);
    image.data[i * 4 + 1] = Math.round(120 + 80 * Math.sin(Math.PI * t));
    image.data[i * 4 + 2] = Math.round(255 - 215 * t);
    image.data[i * 4 + 3] = 255;
  }
  ctx.putImageData(image, 0, 0);
  return { info, canvas };
}
