// Typed mirror of docs/sim-api.md.

export type Box = [[number, number], [number, number]];

export interface MapLoadReport {
  offset: [number, number];
  warnings: string[];
  name: string;
}

// One entry of data/game/maps/index.json (docs/map-format.md).
export interface MapPoolEntry {
  name: string;
  file: string;
  seed: number;
  width: number;
  height: number;
  x_min: number;
  y_min: number;
  source_offset: [number, number];
  entity_count: number;
}

export interface SimConfig {
  vehicle: string;
  fuel: string;
}

export interface CollisionInfo {
  tick: number;
  entity_id: number;
  kind: string;
  damage_taken: number;
}

export interface VehicleState {
  kind: string;
  fuel: string;
  x: number;
  y: number;
  orientation: number;
  speed: number;
  health: number;
  max_health: number;
  destroyed: boolean;
  tick: number;
  collision_box: Box;
  last_collision: CollisionInfo | null;
}

export interface TargetState {
  x: number;
  y: number;
  radius: number;
  distance: number;
  reached: boolean;
}

export interface MapInfo {
  name: string;
  width: number;
  height: number;
  x_min: number;
  y_min: number;
  seed: number;
  source_offset: [number, number];
}

export interface TileInfo {
  id: number;
  name: string;
  color: [number, number, number];
  vehicle_friction_modifier: number;
  is_water: boolean;
}

export interface ResourceInfo {
  id: number;
  name: string;
  color: [number, number, number];
}

export type EntityKind = "tree" | "rock" | "cliff" | "nest";

export interface MapEntity {
  id: number;
  kind: EntityKind;
  name: string;
  x: number;
  y: number;
  collision_box: Box;
  orientation: number;
  cliff_orientation: string | null;
  health: number;
  max_health: number;
  penalty_radius?: number;
}

export interface ObservationSpec {
  local_size: number;
  coarse_size: number;
  coarse_cell_tiles: number;
  channels: string[];
}

export interface VehicleInfo {
  name: string;
  max_health: number;
}

export interface FuelInfo {
  name: string;
  fuel_value: number;
  fuel_acceleration_multiplier: number;
  fuel_top_speed_multiplier: number;
}

// RL episodes (docs/rl.md, "WASM additions").

export type EpisodeStatus = "running" | "success" | "destroyed" | "timeout" | "stalled";
export type PolicyMode = "reactive" | "direct_plan" | "path_planner";

export interface Pose {
  x: number;
  y: number;
  orientation: number;
}

export interface Point {
  x: number;
  y: number;
}

export interface DirectPlanConfig {
  horizon: number;
  execute: number;
}

export interface PathPlannerConfig {
  waypoints: number;
  max_reach: number;
  replan_ticks: number;
  drift_tiles: number;
}

// When an episode succeeds: on entering the target radius, or stopped near its center.
export interface ArrivalRules {
  mode: "enter" | "stop";
  stop_radius: number;
  stop_speed: number;
}

// Rule keys of the episode JSON; `retarget` accepts any subset.
export interface EpisodeRules {
  mode: PolicyMode;
  decision_ticks: number;
  gamma: number;
  reward: Record<string, number>;
  time_limit: { base_ticks: number; per_tile_ticks: number; stall_ticks: number };
  distance_field: { margin: number; tank_tree_cost: number };
  direct_plan: DirectPlanConfig;
  path_planner: PathPlannerConfig;
  arrival: ArrivalRules;
}

// The episode JSON (returned by retarget when the target is reachable).
export interface EpisodeInfo extends EpisodeRules {
  map: string;
  vehicle: string;
  fuel: string;
  start: Pose;
  target: Point;
  radius: number;
  seed?: number;
  optimal_distance: number;
  time_limit_ticks: number;
}

export type RetargetResult = ({ reachable: true } & EpisodeInfo) | { reachable: false };

export interface EpisodeState {
  status: EpisodeStatus;
  ticks: number;
  decisions: number;
  time_limit_ticks: number;
  path_distance: number;
  optimal_distance: number;
  reward: Record<string, number>;
  reward_total: number;
  damage: number;
  collisions: number;
  prev_action: number | null;
}

// Result of decide and follow_path (`replan` only from follow_path).
export interface DecisionResult {
  action: number;
  reward: number;
  terms: Record<string, number>;
  ticks: number;
  status: EpisodeStatus;
  done: boolean;
  replan?: boolean;
}

export interface PolicySpec {
  local_shape: [number, number, number];
  coarse_shape: [number, number, number];
  coarse_cell_tiles: number;
  channels: string[];
  vector_features: string[];
  vector_size: number;
  actions: { count: number; encoding: string; throttle: number[]; steer: number[] };
  reward_terms: string[];
  modes: PolicyMode[];
  vehicles: string[];
  fuels: string[];
  spec_hash: string;
  student: { spec_hash: string; size: number; features: string[] };
}

export interface FieldInfo {
  x_min: number;
  y_min: number;
  width: number;
  height: number;
}

export interface SimApi {
  reset(): void;
  setVehicle(kind: string, fuel: string): void;
  step(throttle: number, steer: number, ticks: number): void;
  vehicleState(): VehicleState;
  targetState(): TargetState | null;
  setTarget(x: number, y: number): void;
  clearTarget(): void;
  mapInfo(): MapInfo;
  tilePalette(): TileInfo[];
  tiles(): Uint8Array;
  resources(): Uint8Array;
  resourcePalette(): ResourceInfo[];
  entitiesInRect(x0: number, y0: number, x1: number, y1: number): MapEntity[];
  loadMap(map: Uint8Array): MapLoadReport;
  mapRevision(): number;
  tileRevision(): number;
  observationSpec(): ObservationSpec;
  observationLocal(): Float32Array;
  observationCoarse(): Float32Array;
  vehicleList(): VehicleInfo[];
  fuelList(): FuelInfo[];

  spec(): PolicySpec;
  retarget(x: number, y: number, rules?: Partial<EpisodeRules>): RetargetResult;
  placeRandom(seed: number): Pose;
  episodeState(): EpisodeState | null;
  policyLocal(): Uint8Array;
  policyCoarse(): Uint8Array;
  policyVector(): Float32Array;
  policyStudent(): Float32Array;
  decide(action: number): DecisionResult;
  followPath(waypoints: Float32Array): DecisionResult;
  planWaypoints(local: Float32Array): Float32Array;
  predictPath(actions: Uint8Array): Float32Array;
  baselinePath(): Float32Array;
  distanceField(): FieldInfo;
  distanceFieldValues(): Float32Array;
}
