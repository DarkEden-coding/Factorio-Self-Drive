import type { Driver, DriveInput } from "../loop";
import type { EpisodeRules, SimApi } from "../sim/types";
import { BaselineDriver } from "./baseline";
import { EpisodeSession } from "./episode";
import { PolicyClient, PolicyDriver, type ConnectionState } from "./policyClient";
import { encodeAction } from "./types";

export type DriverKind = "policy" | "baseline" | "manual";

export const POLICY_URL = "ws://localhost:8765";
export const SERVE_COMMAND = "cd python && uv run fsim-rl serve --checkpoint runs/reactive-1/best/model.pt";

// Ticks per loop step without an episode (braking, idling, overriding with no target).
const IDLE_TICKS = 4;
// How long a rejected click shows "UNREACHABLE".
const NOTICE_MS = 1500;

export interface SessionEvents {
  // One-line status of the session (target set, reached, unreachable, ...).
  onStatus: (text: string) => void;
  onConnection: (state: ConnectionState, message: string) => void;
}

// The viewer's one live session. Clicking sets the target from wherever the vehicle is;
// the selected driver drives there one decision at a time. Held drive keys override the
// driver. Without a running episode the vehicle brakes to a stop and idles.
// It is the loop's only driver.
export class LiveSession implements Driver {
  driverKind: DriverKind = "policy";
  readonly session: EpisodeSession;
  readonly client: PolicyClient;
  // Short-lived message for the banner, e.g. "UNREACHABLE".
  notice: { text: string; until: number } | null = null;
  private baseline: BaselineDriver;
  private policy: PolicyDriver;
  private overriding = false;
  private lastStatus: string | null = null;

  constructor(
    private sim: SimApi,
    private readInput: () => DriveInput,
    private events: SessionEvents,
  ) {
    this.session = new EpisodeSession(sim);
    const spec = sim.spec();
    this.client = new PolicyClient({ full: spec.spec_hash, student: spec.student.spec_hash, studentSmooth: spec.student.smooth_spec_hash }, {
      onState: (state, message) => {
        events.onConnection(state, message);
        if (state !== "ready") return;
        this.matchPolicyRules();
        if (this.driverKind === "policy" && this.session.running) events.onStatus("Policy connected: driving to the target.");
      },
    });
    this.baseline = new BaselineDriver(this.session);
    this.policy = new PolicyDriver(sim, this.session, this.client);
  }

  connect(): void {
    this.client.connect(POLICY_URL);
  }

  setDriver(kind: DriverKind): void {
    this.driverKind = kind;
    this.baseline.interrupt();
    this.policy.interrupt();
    if (kind === "policy") this.matchPolicyRules();
    this.events.onStatus(this.waitingReason() ?? `Driver: ${kind}`);
  }

  // A click on the world. Keeps the vehicle's state; restarts the episode bookkeeping.
  setTarget(x: number, y: number, now: number): void {
    if (this.sim.vehicleState().destroyed) {
      this.events.onStatus("The vehicle is destroyed. Press R for a new start.");
      return;
    }
    const result = this.session.retarget(x, y, this.policyRules());
    if (!result.reachable) {
      this.notice = { text: "UNREACHABLE", until: now + NOTICE_MS };
      this.events.onStatus(`Unreachable target (${x.toFixed(1)}, ${y.toFixed(1)}); kept the previous one.`);
      return;
    }
    this.lastStatus = "running";
    this.events.onStatus(
      this.waitingReason() ??
        `Driving to (${x.toFixed(1)}, ${y.toFixed(1)}): ${result.optimal_distance.toFixed(1)} tiles by path, ` +
          `limit ${result.time_limit_ticks} ticks`,
    );
  }

  // R: a new random start pose on the current map; clears the target.
  placeRandom(): void {
    const pose = this.sim.placeRandom(Math.floor(Math.random() * 2 ** 32));
    this.session.clear();
    this.lastStatus = null;
    this.events.onStatus(`New start at (${pose.x.toFixed(1)}, ${pose.y.toFixed(1)}). Click the map to set a target.`);
  }

  // Called once per frame before the loop runs.
  frame(): void {
    this.session.sync();
    const status = this.session.state?.status ?? null;
    if (status !== this.lastStatus) {
      this.lastStatus = status;
      if (status && status !== "running") this.events.onStatus(endMessage(status));
    }
  }

  // The banner notice, if still showing.
  activeNotice(now: number): string | null {
    return this.notice && now < this.notice.until ? this.notice.text : null;
  }

  decide(): number {
    const s = this.session;
    const input = this.readInput();
    const ticks = s.info?.decision_ticks ?? IDLE_TICKS;
    if (input.throttle !== 0 || input.steer !== 0) {
      // The player drives. Episode decisions keep the bookkeeping going.
      if (!this.overriding) {
        this.overriding = true;
        this.baseline.interrupt();
        this.policy.interrupt();
      }
      if (s.running) return s.decide(encodeAction(input.throttle, input.steer)).ticks;
      this.sim.step(input.throttle, input.steer, ticks);
      return ticks;
    }
    this.overriding = false;
    if (!s.running) return this.brakeOrIdle(ticks);
    switch (this.driverKind) {
      case "policy":
        // Without a server the car stops; waiting for a reply pauses the clock.
        return this.client.state === "ready" ? this.policy.decide() : this.brakeOrIdle(ticks);
      case "baseline":
        return this.baseline.decide();
      case "manual":
        return s.decide(encodeAction(0, 0)).ticks;
    }
  }

  // Why the selected driver can't drive right now, if it can't.
  waitingReason(): string | null {
    if (this.driverKind !== "policy" || this.client.state === "ready") return null;
    return `Policy server not connected: the car waits. Start it with \`${SERVE_COMMAND}\` and press Reconnect, or pick another driver.`;
  }

  // Brakes until stopped (never into reverse), then lets ticks pass.
  private brakeOrIdle(ticks: number): number {
    for (let i = 0; i < ticks; i++) {
      const speed = this.sim.vehicleState().speed;
      this.sim.step(speed > 0 ? -1 : speed < 0 ? 1 : 0, 0, 1);
    }
    return ticks;
  }

  // The server's action mode and settings, which an episode needs for the policy to drive it.
  private policyRules(): Partial<EpisodeRules> | undefined {
    const hello = this.client.hello;
    if (this.driverKind !== "policy" || !hello) return undefined;
    const rules: Partial<EpisodeRules> = { mode: hello.mode, decision_ticks: hello.decision_ticks };
    if (hello.mode === "direct_plan") rules.direct_plan = hello.mode_config as EpisodeRules["direct_plan"];
    if (hello.mode === "path_planner") rules.path_planner = hello.mode_config as EpisodeRules["path_planner"];
    if (hello.arrival) rules.arrival = hello.arrival;
    if (hello.field) rules.distance_field = hello.field;
    return rules;
  }

  // Retargets the running episode under the server's rules if they differ.
  private matchPolicyRules(): void {
    const info = this.session.info;
    const hello = this.client.hello;
    if (this.driverKind !== "policy" || !hello || !info || !this.session.running) return;
    const arrival = hello.arrival?.mode ?? "enter";
    if (info.mode !== hello.mode || info.decision_ticks !== hello.decision_ticks || info.arrival.mode !== arrival ||
      (hello.field && JSON.stringify(info.distance_field) !== JSON.stringify(hello.field))) {
      this.session.retarget(info.target.x, info.target.y, this.policyRules());
    }
  }
}

function endMessage(status: "success" | "destroyed" | "timeout" | "stalled"): string {
  switch (status) {
    case "success":
      return "Reached the target. Click the map for the next one.";
    case "destroyed":
      return "Destroyed. Press R for a new start.";
    case "timeout":
      return "Timed out. Click a target to retry from here.";
    case "stalled":
      return "Stalled (no progress). Click a target to retry from here.";
  }
}
