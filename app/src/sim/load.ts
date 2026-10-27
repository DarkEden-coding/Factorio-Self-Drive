import type { SimApi, SimConfig } from "./types";
import { wrapWasmSim, type RawSim } from "./wasm";

interface WasmModule {
  default: () => Promise<unknown>;
  Sim: new (config_json: string, map: Uint8Array) => RawSim;
}

// A glob (instead of a plain import) so a missing package gives a clear message
// instead of a Vite resolve error.
const wasmModules = import.meta.glob("../../../crates/fsim-wasm/pkg/fsim_wasm.js");

export async function createSim(config: SimConfig, map: Uint8Array): Promise<SimApi> {
  const loadModule = Object.values(wasmModules)[0];
  if (!loadModule) {
    throw new Error("crates/fsim-wasm/pkg not found. Build it with: npm run wasm");
  }
  const mod = (await loadModule()) as WasmModule;
  await mod.default();
  return wrapWasmSim(new mod.Sim(JSON.stringify(config), map));
}
