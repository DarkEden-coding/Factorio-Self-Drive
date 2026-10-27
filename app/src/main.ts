import "./style.css";
import { Camera } from "./camera";
import { bindWorldPointer, KeyInput } from "./input";
import { SimLoop, SPEEDS } from "./loop";
import { FrameStats } from "./perf";
import { ASSETS_HINT, GameAssets } from "./render/assets";
import { Minimap } from "./render/minimap";
import { TileCache } from "./render/tileCache";
import { WorldRenderer } from "./render/world";
import { LiveSession } from "./rl/controller";
import { drawRlOverlays } from "./rl/overlays";
import { defaultSettings } from "./settings";
import { createSim } from "./sim/load";
import { fetchMapBytes, fetchPoolIndex } from "./sim/maps";
import type { MapPoolEntry, SimConfig } from "./sim/types";
import { DriverControls } from "./ui/driverControls";
import { buildHelp } from "./ui/help";
import { Hud } from "./ui/hud";
import { ObsView } from "./ui/obsView";
import { Panel } from "./ui/panel";
import { PolicyPanel } from "./ui/policyPanel";

const SPEED_LABELS = SPEEDS.map((s) => (s === Infinity ? "max" : `${s}x`));

function byId<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

// Lets the browser paint a status message before a long synchronous call.
const nextPaint = () => new Promise<void>((resolve) => requestAnimationFrame(() => setTimeout(resolve)));

async function main(): Promise<void> {
  let config: SimConfig = { vehicle: "car", fuel: "coal" };
  const maps = await fetchPoolIndex();
  const [initialMap, assets] = await Promise.all([fetchMapBytes(maps[0]), GameAssets.load()]);
  const sim = await createSim(config, initialMap);
  if (!assets) console.info(ASSETS_HINT);

  const settings = defaultSettings();
  settings.gameGraphics = assets !== null;
  const camera = new Camera();
  const tiles = new TileCache();
  const worldCanvas = byId<HTMLCanvasElement>("world");
  const renderer = new WorldRenderer(worldCanvas, sim, tiles, settings, assets);
  const keys = new KeyInput(onKeyPress);
  const loop = new SimLoop(sim, () => keys.read());
  const hud = new Hud(byId("hud"), byId("banner"));
  const stats = new FrameStats();
  const help = byId("help");
  buildHelp(help);

  const minimap = new Minimap(byId<HTMLCanvasElement>("minimap"), tiles, (x, y) => {
    settings.freeCamera = true;
    panel.syncToggles();
    camera.x = x;
    camera.y = y;
  });

  async function loadPoolMap(entry: MapPoolEntry, progress: (text: string) => void): Promise<string> {
    progress(`Fetching ${entry.name}...`);
    const bytes = await fetchMapBytes(entry, (f) => progress(`Fetching ${entry.name} ${Math.round(f * 100)}%`));
    return applyMap(bytes, progress);
  }

  async function applyMap(bytes: Uint8Array, progress: (text: string) => void): Promise<string> {
    progress("Parsing map...");
    await nextPaint();
    const report = sim.loadMap(bytes);
    newStart();
    if (report.warnings.length) console.warn("Map load warnings", report.warnings);
    const [ox, oy] = report.offset;
    const warnings = report.warnings.length ? `, ${report.warnings.length} warnings (see console)` : "";
    return `Loaded ${report.name}. Factorio = sim + (${ox}, ${oy})${warnings}`;
  }

  const panel = new Panel(byId("panel"), {
    vehicles: sim.vehicleList().map((v) => v.name),
    fuels: sim.fuelList().map((f) => f.name),
    config,
    settings,
    speeds: SPEED_LABELS,
    maps,
    mapIndex: 0,
    hasGameAssets: assets !== null,
    onVehicleChange: (vehicle, fuel) => {
      config = { vehicle, fuel };
      sim.setVehicle(vehicle, fuel);
    },
    onReset: () => newStart(),
    onLoadPoolMap: loadPoolMap,
    onLoadMapFile: (bytes, _name, progress) => applyMap(bytes, progress),
    onSpeedChange: setSpeed,
  });
  panel.setStatus(`Loaded ${maps[0].name}`);
  const spec = sim.observationSpec();
  const obsView = new ObsView(panel.obsRoot, sim, spec, sim.spec());

  const live = new LiveSession(sim, () => keys.read(), {
    onStatus: (text) => driverControls.setStatus(text),
    onConnection: (state, message) => driverControls.setConnection(state, message),
  });
  const driverControls = new DriverControls(panel.driverRoot, live);
  const policyPanel = new PolicyPanel(panel.policyRoot, live.session);
  loop.driver = live;

  // A random start pose on the current map, with the camera following the vehicle.
  function newStart(): void {
    live.placeRandom();
    settings.freeCamera = false;
    panel.syncToggles();
    const v = sim.vehicleState();
    camera.x = v.x;
    camera.y = v.y;
  }

  function setSpeed(index: number): void {
    loop.speed = SPEEDS[index];
    panel.setSpeedIndex(index);
  }

  function toggle(key: "showBoxes" | "showGrid" | "showObs" | "freeCamera" | "gameGraphics"): void {
    if (key === "gameGraphics" && !assets) return;
    settings[key] = !settings[key];
    panel.syncToggles();
  }

  function onKeyPress(code: string): void {
    if (code === "KeyH") help.hidden = !help.hidden;
    else if (code === "Escape") help.hidden = true;
    else if (code === "KeyP") loop.paused = !loop.paused;
    else if (code === "Period") loop.stepOnce();
    else if (code === "KeyR") newStart();
    else if (code === "KeyB") toggle("showBoxes");
    else if (code === "KeyG") toggle("showGrid");
    else if (code === "KeyO") toggle("showObs");
    else if (code === "KeyF") toggle("freeCamera");
    else if (code === "KeyT") toggle("gameGraphics");
    else if (/^Digit[1-5]$/.test(code)) setSpeed(Number(code.slice(5)) - 1);
  }

  bindWorldPointer(worldCanvas, camera, () => settings.freeCamera, (x, y) => live.setTarget(x, y, performance.now()));
  new ResizeObserver(() => renderer.resize(camera)).observe(worldCanvas);
  renderer.resize(camera);
  newStart();
  live.connect();

  let last = performance.now();
  function frame(now: number): void {
    const start = performance.now();
    const dt = Math.min(now - last, 250);
    last = now;
    live.frame();
    loop.frame(dt);

    const vehicle = sim.vehicleState();
    const target = sim.targetState();
    if (!settings.freeCamera) {
      camera.x = vehicle.x;
      camera.y = vehicle.y;
    }
    tiles.ensure(sim);
    const episode = live.session;
    renderer.draw(camera, vehicle, target, spec, (ctx, zoom) => drawRlOverlays(ctx, episode, settings, zoom), episode.info !== null);
    minimap.draw(camera, vehicle, target);
    hud.update(vehicle, target, {
      paused: loop.paused,
      speedLabel: SPEED_LABELS[SPEEDS.indexOf(loop.speed)],
      freeCamera: settings.freeCamera,
      perf: stats.label(),
      episodeStatus: episode.state?.status ?? null,
      notice: live.activeNotice(now),
    });
    obsView.maybeUpdate(now, episode.info !== null);
    policyPanel.update(now);
    stats.record(now, performance.now() - start);
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}

main().catch((err) => {
  console.error(err);
  document.body.textContent = `Failed to start: ${String(err)}`;
});
