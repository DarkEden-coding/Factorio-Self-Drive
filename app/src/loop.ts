import type { SimApi } from "./sim/types";

export const TICK_MS = 1000 / 60;
export const SPEEDS = [1, 2, 4, 8, Infinity];

const MAX_SPEED_BUDGET_MS = 12;
const MAX_SPEED_BATCH = 16;
const MAX_CATCH_UP_MS = 100;

export interface DriveInput {
  throttle: number;
  steer: number;
}

// Drives the sim one RL decision at a time instead of tick by tick.
export interface Driver {
  // Runs one decision and returns the ticks it simulated, or 0 when it can't run now
  // (waiting for a reply from the policy server).
  decide(): number;
}

// Fixed 60 Hz simulation clock, decoupled from rendering. `frame` is called once per
// rendered frame with the elapsed wall time. With a driver the clock advances in whole
// decisions, so a decision can run up to `decision_ticks - 1` ticks ahead of wall time.
export class SimLoop {
  paused = false;
  speed = 1;
  driver: Driver | null = null;
  private accumulator = 0;

  constructor(
    private sim: SimApi,
    private readInput: () => DriveInput,
  ) {}

  frame(dtMs: number): void {
    if (this.paused) {
      this.accumulator = 0;
      return;
    }
    if (this.speed === Infinity) {
      const start = performance.now();
      while (performance.now() - start < MAX_SPEED_BUDGET_MS) {
        if (this.advance(MAX_SPEED_BATCH) === 0) break;
        if (!this.driver && this.sim.vehicleState().destroyed) break;
      }
      return;
    }
    this.accumulator = Math.min(this.accumulator + dtMs * this.speed, MAX_CATCH_UP_MS * this.speed);
    const ticks = Math.floor(this.accumulator / TICK_MS);
    if (ticks > 0) this.accumulator -= this.advance(ticks) * TICK_MS;
  }

  // One tick, or one decision with a driver.
  stepOnce(): void {
    if (this.driver) this.driver.decide();
    else this.advance(1);
  }

  // Simulates about `ticks` ticks and returns how many ran: more when the last decision
  // overshoots, fewer when the driver has to wait.
  private advance(ticks: number): number {
    if (!this.driver) {
      const { throttle, steer } = this.readInput();
      this.sim.step(throttle, steer, ticks);
      return ticks;
    }
    let done = 0;
    while (done < ticks) {
      const t = this.driver.decide();
      if (t === 0) break;
      done += t;
    }
    return done;
  }
}
