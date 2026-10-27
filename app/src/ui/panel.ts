import type { Settings } from "../settings";
import type { MapPoolEntry, SimConfig } from "../sim/types";
import { ASSETS_HINT } from "../render/assets";
import { el, fillSelect } from "./dom";

export interface PanelOptions {
  vehicles: string[];
  fuels: string[];
  config: SimConfig;
  settings: Settings;
  speeds: string[];
  maps: MapPoolEntry[];
  mapIndex: number;
  // False when /game-assets/manifest.json is missing.
  hasGameAssets: boolean;
  onVehicleChange: (vehicle: string, fuel: string) => void;
  onReset: () => void;
  // Both resolve to a one-line status. `progress` updates the status while loading.
  onLoadPoolMap: (entry: MapPoolEntry, progress: (text: string) => void) => Promise<string>;
  onLoadMapFile: (bytes: Uint8Array, name: string, progress: (text: string) => void) => Promise<string>;
  onSpeedChange: (index: number) => void;
}

const mapLabel = (m: MapPoolEntry) => `${m.name} - seed ${m.seed}, ${m.entity_count.toLocaleString("en-US")} entities`;

export class Panel {
  readonly obsRoot = el("div", { className: "obs-root" });
  readonly driverRoot = el("div");
  readonly policyRoot = el("div");
  private vehicleSelect = el("select");
  private fuelSelect = el("select");
  private checkboxes = new Map<keyof Settings, HTMLInputElement>();
  private speedSelect = el("select");
  private mapSelect = el("select", { className: "map-select" });
  private localOption: HTMLOptionElement | null = null;
  private mapStatus = el("div", { className: "map-status" });
  private busy = false;
  // Select value of the map that is actually loaded ("" = a local file).
  private shown: string;

  constructor(
    root: HTMLElement,
    private opts: PanelOptions,
  ) {
    this.shown = String(opts.mapIndex);
    const vehicle = this.vehicleSelect;
    const fuel = this.fuelSelect;
    fillSelect(vehicle, opts.vehicles, opts.config.vehicle);
    fillSelect(fuel, opts.fuels, opts.config.fuel);
    fillSelect(this.speedSelect, opts.speeds, opts.speeds[0]);
    this.mapSelect.replaceChildren(...opts.maps.map((m, i) => el("option", { value: String(i), textContent: mapLabel(m) })));
    this.mapSelect.value = String(opts.mapIndex);

    const vehicleChanged = () => {
      opts.onVehicleChange(vehicle.value, fuel.value);
      vehicle.blur();
      fuel.blur();
    };
    vehicle.addEventListener("change", vehicleChanged);
    fuel.addEventListener("change", vehicleChanged);
    this.speedSelect.addEventListener("change", () => {
      opts.onSpeedChange(this.speedSelect.selectedIndex);
      this.speedSelect.blur();
    });

    this.mapSelect.addEventListener("change", async () => {
      this.mapSelect.blur();
      if (this.mapSelect.value === "") return;
      const next = this.mapSelect.value;
      if (await this.run(() => opts.onLoadPoolMap(opts.maps[Number(next)], (t) => this.setStatus(t)))) {
        this.shown = next;
        this.removeLocalOption();
      } else {
        // A failed load leaves the previous map running; show it as selected again.
        this.mapSelect.value = this.shown;
      }
    });

    const mapFile = el("input", { type: "file", accept: ".fsmap", hidden: true });
    mapFile.addEventListener("change", async () => {
      const file = mapFile.files?.[0];
      mapFile.value = "";
      if (!file) return;
      const ok = await this.run(async () => {
        this.setStatus(`Reading ${file.name}...`);
        const bytes = new Uint8Array(await file.arrayBuffer());
        return opts.onLoadMapFile(bytes, file.name, (t) => this.setStatus(t));
      });
      if (ok) {
        this.showLocalOption(file.name);
        this.shown = "";
      }
    });

    const gameBox = this.toggle("gameGraphics", "Game graphics (T)", !opts.hasGameAssets);
    root.append(
      el("h2", { textContent: "Simulator" }),
      this.field("Vehicle", vehicle),
      this.field("Fuel", fuel),
      this.field("Sim speed", this.speedSelect),
      el("h3", { textContent: "Map" }),
      this.mapSelect,
      el("div", { className: "buttons" }, [
        el("button", {
          textContent: "New start (R)",
          onclick: (e: MouseEvent) => {
            opts.onReset();
            (e.currentTarget as HTMLElement).blur();
          },
        }),
        el("button", { textContent: "Load map file", onclick: () => mapFile.click() }),
      ]),
      mapFile,
      this.mapStatus,
      this.driverRoot,
      this.policyRoot,
      el("h3", { textContent: "View" }),
      gameBox,
      ...(opts.hasGameAssets ? [] : [el("div", { className: "map-status", textContent: ASSETS_HINT })]),
      this.toggle("showBoxes", "Collision boxes (B)"),
      this.toggle("showGrid", "Grid lines (G)"),
      this.toggle("showObs", "Observation overlay (O)"),
      this.toggle("freeCamera", "Free camera, drag to pan (F)"),
      el("h4", { textContent: "Episode overlays" }),
      this.toggle("showTrail", "Trail"),
      this.toggle("showPlan", "Plan (waypoints / predicted path)"),
      this.toggle("showBaseline", "A* baseline path"),
      this.toggle("showField", "Distance field heatmap"),
      this.obsRoot,
    );
  }

  setStatus(text: string): void {
    this.mapStatus.textContent = text;
  }

  setSpeedIndex(index: number): void {
    this.speedSelect.selectedIndex = index;
  }

  syncToggles(): void {
    for (const [key, box] of this.checkboxes) box.checked = this.opts.settings[key];
  }

  // Runs a load with the map controls locked. Returns false if it failed.
  private async run(load: () => Promise<string>): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    this.mapSelect.disabled = true;
    try {
      this.setStatus(await load());
      return true;
    } catch (err) {
      this.setStatus(`Load failed: ${String(err)}`);
      return false;
    } finally {
      this.busy = false;
      this.mapSelect.disabled = false;
    }
  }

  private showLocalOption(name: string): void {
    this.removeLocalOption();
    this.localOption = el("option", { value: "", textContent: `local file: ${name}` });
    this.mapSelect.append(this.localOption);
    this.mapSelect.value = "";
  }

  private removeLocalOption(): void {
    this.localOption?.remove();
    this.localOption = null;
  }

  private field(label: string, input: HTMLElement): HTMLElement {
    return el("label", { className: "field" }, [el("span", { textContent: label }), input]);
  }

  private toggle(key: keyof Settings, label: string, disabled = false): HTMLElement {
    const box = el("input", { type: "checkbox", checked: this.opts.settings[key], disabled });
    box.addEventListener("change", () => {
      this.opts.settings[key] = box.checked;
      box.blur();
    });
    this.checkboxes.set(key, box);
    return el("label", { className: "toggle" }, [box, el("span", { textContent: label })]);
  }
}
