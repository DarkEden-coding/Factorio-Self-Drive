import type {
  DecisionResult,
  EpisodeState,
  FieldInfo,
  FuelInfo,
  MapEntity,
  MapInfo,
  MapLoadReport,
  ObservationSpec,
  PolicySpec,
  Pose,
  ResourceInfo,
  RetargetResult,
  SimApi,
  TargetState,
  TileInfo,
  VehicleInfo,
  VehicleState,
} from "./types";

// The wasm-bindgen `Sim` class as described in docs/sim-api.md.
export interface RawSim {
  reset(): void;
  set_vehicle(kind: string, fuel: string): void;
  step(throttle: number, steer: number, ticks: number): void;
  vehicle_state(): string;
  target_state(): string;
  set_target(x: number, y: number): void;
  clear_target(): void;
  map_info(): string;
  tile_palette(): string;
  tiles(): Uint8Array;
  resources(): Uint8Array;
  resource_palette(): string;
  entities_in_rect(x0: number, y0: number, x1: number, y1: number): string;
  load_map(map: Uint8Array): string;
  map_revision(): number;
  tile_revision(): number;
  observation_spec(): string;
  observation_local(): Float32Array;
  observation_coarse(): Float32Array;
  vehicle_list(): string;
  fuel_list(): string;
  spec(): string;
  retarget(x: number, y: number, radius?: number, rules_json?: string): string;
  place_random(seed: number): string;
  episode_state(): string;
  policy_local(): Uint8Array;
  policy_coarse(): Uint8Array;
  policy_vector(): Float32Array;
  policy_student(): Float32Array;
  decide(action: number): string;
  follow_path(waypoints: Float32Array): string;
  plan_waypoints(local: Float32Array): Float32Array;
  predict_path(actions: Uint8Array): Float32Array;
  baseline_path(): Float32Array;
  distance_field(): string;
  distance_field_values(): Float32Array;
}

export function wrapWasmSim(raw: RawSim): SimApi {
  return {
    reset: () => raw.reset(),
    setVehicle: (kind, fuel) => raw.set_vehicle(kind, fuel),
    step: (throttle, steer, ticks) => raw.step(throttle, steer, ticks),
    vehicleState: () => JSON.parse(raw.vehicle_state()) as VehicleState,
    targetState: () => JSON.parse(raw.target_state()) as TargetState | null,
    setTarget: (x, y) => raw.set_target(x, y),
    clearTarget: () => raw.clear_target(),
    mapInfo: () => JSON.parse(raw.map_info()) as MapInfo,
    tilePalette: () => JSON.parse(raw.tile_palette()) as TileInfo[],
    tiles: () => raw.tiles(),
    resources: () => raw.resources(),
    resourcePalette: () => JSON.parse(raw.resource_palette()) as ResourceInfo[],
    entitiesInRect: (x0, y0, x1, y1) =>
      JSON.parse(raw.entities_in_rect(x0, y0, x1, y1)) as MapEntity[],
    loadMap: (map) => JSON.parse(raw.load_map(map)) as MapLoadReport,
    mapRevision: () => raw.map_revision(),
    tileRevision: () => raw.tile_revision(),
    observationSpec: () => JSON.parse(raw.observation_spec()) as ObservationSpec,
    observationLocal: () => raw.observation_local(),
    observationCoarse: () => raw.observation_coarse(),
    vehicleList: () => JSON.parse(raw.vehicle_list()) as VehicleInfo[],
    fuelList: () => JSON.parse(raw.fuel_list()) as FuelInfo[],
    spec: () => JSON.parse(raw.spec()) as PolicySpec,
    retarget: (x, y, rules) =>
      JSON.parse(raw.retarget(x, y, undefined, rules ? JSON.stringify(rules) : undefined)) as RetargetResult,
    placeRandom: (seed) => JSON.parse(raw.place_random(seed)) as Pose,
    episodeState: () => JSON.parse(raw.episode_state()) as EpisodeState | null,
    policyLocal: () => raw.policy_local(),
    policyCoarse: () => raw.policy_coarse(),
    policyVector: () => raw.policy_vector(),
    policyStudent: () => raw.policy_student(),
    decide: (action) => JSON.parse(raw.decide(action)) as DecisionResult,
    followPath: (waypoints) => JSON.parse(raw.follow_path(waypoints)) as DecisionResult,
    planWaypoints: (local) => raw.plan_waypoints(local),
    predictPath: (actions) => raw.predict_path(actions),
    baselinePath: () => raw.baseline_path(),
    distanceField: () => JSON.parse(raw.distance_field()) as FieldInfo,
    distanceFieldValues: () => raw.distance_field_values(),
  };
}
