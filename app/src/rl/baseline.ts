import type { Driver } from "../loop";
import type { EpisodeSession } from "./episode";

// The in-browser classical baseline: the A* path down the episode's distance field,
// followed by the Rust pure-pursuit controller until it asks for a replan.
export class BaselineDriver implements Driver {
  private path: Float32Array | null = null;
  private generation = -1;

  constructor(private session: EpisodeSession) {}

  // Drops the current path, e.g. after the player drove the vehicle off it.
  interrupt(): void {
    this.path = null;
  }

  decide(): number {
    const s = this.session;
    if (!s.running) return 0;
    if (this.generation !== s.generation) {
      this.generation = s.generation;
      this.path = null;
    }
    if (!this.path) {
      this.path = s.baselinePath();
      if (!this.path || this.path.length < 4) return 0;
      s.plan = { kind: "waypoints", points: this.path, startDecision: s.decisions };
    }
    const result = s.followPath(this.path);
    if (result.replan) this.path = null;
    return result.ticks;
  }
}
