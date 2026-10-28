import type { Driver } from "../loop";
import type { SimApi } from "../sim/types";
import type { EpisodeSession } from "./episode";
import type { ActionMessage, HelloMessage, ServerMessage } from "./types";

export type ConnectionState = "disconnected" | "connecting" | "ready" | "refused" | "error";

export interface ClientEvents {
  // Connection state changed; `message` explains refusals, errors and disconnects.
  onState: (state: ConnectionState, message: string) => void;
}

function toBase64(bytes: Uint8Array): string {
  let text = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    text += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(text);
}

// WebSocket client for `fsim-rl serve`. One request is in flight at a time, and the
// server answers in order, so a reply always belongs to the last request.
export class PolicyClient {
  state: ConnectionState = "disconnected";
  hello: HelloMessage | null = null;
  private ws: WebSocket | null = null;
  private pending: ((reply: ActionMessage) => void) | null = null;

  constructor(
    private specHashes: { full: string; student: string; studentSmooth: string },
    private events: ClientEvents,
  ) {}

  get busy(): boolean {
    return this.pending !== null;
  }

  connect(url: string): void {
    this.close();
    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch (err) {
      this.setState("error", `Bad URL: ${String(err)}`);
      return;
    }
    this.ws = ws;
    this.setState("connecting", `Connecting to ${url}...`);
    ws.onmessage = (e) => this.receive(ws, String(e.data));
    ws.onclose = (e) => {
      if (this.ws !== ws) return;
      this.ws = null;
      this.pending = null;
      if (this.state === "refused" || this.state === "error") return;
      const why = this.state === "connecting" ? `Could not connect to ${url}` : "Server disconnected";
      this.setState("disconnected", e.reason ? `${why}: ${e.reason}` : why);
    };
  }

  close(): void {
    const ws = this.ws;
    this.ws = null;
    this.pending = null;
    this.hello = null;
    ws?.close();
    if (ws) this.setState("disconnected", "Disconnected");
  }

  // Sends the current policy observation. Returns false when not ready or already waiting.
  request(sim: SimApi, onReply: (reply: ActionMessage) => void): boolean {
    if (this.state !== "ready" || !this.ws || this.pending) return false;
    this.pending = onReply;
    const message =
      this.hello?.observation === "student"
        ? { type: "act", student: Array.from(this.hello.route_source === "field_smooth" ? sim.policyStudentSmooth() : sim.policyStudent()) }
        : {
            type: "act",
            local: toBase64(sim.policyLocal()),
            coarse: toBase64(sim.policyCoarse()),
            vector: Array.from(sim.policyVector()),
          };
    this.ws.send(JSON.stringify(message));
    return true;
  }

  private receive(ws: WebSocket, raw: string): void {
    if (this.ws !== ws) return;
    let message: ServerMessage;
    try {
      message = JSON.parse(raw) as ServerMessage;
    } catch {
      this.fail("Server sent invalid JSON");
      return;
    }
    if (message.type === "hello") {
      const observation = message.observation ?? "full";
      const expected = observation === "student" && message.route_source === "field_smooth"
        ? this.specHashes.studentSmooth : this.specHashes[observation];
      if (message.spec_hash !== expected) {
        this.state = "refused";
        ws.close();
        this.setState(
          "refused",
          `Refused: server ${observation} spec_hash ${message.spec_hash} differs from this build's ${expected}. ` +
            "Rebuild the wasm package or retrain the checkpoint.",
        );
        return;
      }
      this.hello = message;
      const student = observation === "student" ? " (student)" : "";
      this.setState("ready", `Connected: ${message.mode}${student}, ${message.decision_ticks} ticks/decision`);
    } else if (message.type === "action") {
      const reply = this.pending;
      this.pending = null;
      reply?.(message);
    } else if (message.type === "error") {
      this.fail(`Server error: ${message.message}`);
    }
  }

  private fail(message: string): void {
    this.state = "error";
    this.pending = null;
    this.ws?.close();
    this.setState("error", message);
  }

  private setState(state: ConnectionState, message: string): void {
    this.state = state;
    this.events.onState(state, message);
  }
}

// Executes server actions like the env does (docs/rl.md "Policy server"): `reactive` runs
// one decision per call; `direct_plan` runs the first `execute` actions of the plan;
// `path_planner` follows the converted waypoints until the controller asks to replan.
export class PolicyDriver implements Driver {
  private queue: number[] = [];
  private waypoints: Float32Array | null = null;
  private generation = -1;
  // Generation of the request in flight; replies from an earlier episode or from
  // before an interrupt are dropped.
  private requested = -1;

  constructor(
    private sim: SimApi,
    private session: EpisodeSession,
    private client: PolicyClient,
  ) {}

  // Forgets queued actions, the plan and the request in flight (their observation is
  // stale once the player has driven).
  interrupt(): void {
    this.queue = [];
    this.waypoints = null;
    this.requested = -1;
  }

  decide(): number {
    const s = this.session;
    if (!s.running || !s.info) return 0;
    if (this.generation !== s.generation) {
      this.generation = s.generation;
      this.queue = [];
      this.waypoints = null;
    }
    const next = this.queue.shift();
    if (next !== undefined) return s.decide(next).ticks;
    if (this.waypoints) {
      const result = s.followPath(this.waypoints);
      if (result.replan) this.waypoints = null;
      return result.ticks;
    }
    if (!this.client.busy) {
      this.requested = s.generation;
      this.client.request(this.sim, (reply) => this.accept(reply));
    }
    return 0;
  }

  private accept(reply: ActionMessage): void {
    const s = this.session;
    if (this.requested !== s.generation || !s.running || !s.info) return;
    const decision = s.decisions;
    s.policy = { decision, probs: reply.probs, value: reply.value };
    const action = reply.action;
    if (s.info.mode === "reactive" && typeof action === "number") {
      this.queue = [action];
      if (this.client.hello?.route_source === "field_smooth") {
        s.plan = { kind: "waypoints", points: this.sim.smoothPath(), startDecision: decision };
      }
    } else if (s.info.mode === "direct_plan" && Array.isArray(action)) {
      const plan = action as number[];
      this.queue = plan.slice(0, s.info.direct_plan.execute);
      s.plan = { kind: "predicted", points: this.sim.predictPath(Uint8Array.from(plan)), startDecision: decision };
    } else if (s.info.mode === "path_planner" && Array.isArray(action)) {
      const local = Float32Array.from((action as [number, number][]).flat());
      this.waypoints = this.sim.planWaypoints(local);
      s.plan = { kind: "waypoints", points: this.waypoints, startDecision: decision };
    } else {
      console.warn("Policy action does not match the episode mode", s.info.mode, action);
    }
  }
}
