// Policy server messages (docs/rl.md).
import type { ArrivalRules, DirectPlanConfig, PathPlannerConfig, PolicyMode } from "../sim/types";

export interface HelloMessage {
  type: "hello";
  mode: PolicyMode;
  decision_ticks: number;
  spec_hash: string;
  // Which observation the model reads; servers before student support omit it ("full").
  observation?: "full" | "student";
  model: Record<string, unknown>;
  mode_config: Partial<DirectPlanConfig & PathPlannerConfig>;
  // The model's arrival rule; servers before stop arrival omit it ("enter").
  arrival?: ArrivalRules;
}

export interface ActionMessage {
  type: "action";
  action: number | number[] | [number, number][];
  probs: number[] | null;
  value: number;
}

export interface ErrorMessage {
  type: "error";
  message: string;
}

export type ServerMessage = HelloMessage | ActionMessage | ErrorMessage;

// Model outputs for the policy panel.
export interface PolicyOutput {
  decision: number;
  probs: number[] | null;
  value: number | null;
}

// What the current plan overlay draws. `points` are world x,y pairs; `startDecision`
// is the decision the plan was made at (its executed part is the trail from there).
export interface PlanOverlay {
  kind: "waypoints" | "predicted";
  points: Float32Array;
  startDecision: number;
}

export const ACTION_COUNT = 9;

// index = (throttle + 1) * 3 + (steer + 1)
export function encodeAction(throttle: number, steer: number): number {
  return (throttle + 1) * 3 + (steer + 1);
}

const THROTTLE_NAMES = ["reverse", "coast", "forward"];
const STEER_NAMES = ["left", "straight", "right"];

export function actionLabel(action: number): string {
  return `${THROTTLE_NAMES[Math.floor(action / 3)]} ${STEER_NAMES[action % 3]}`;
}
