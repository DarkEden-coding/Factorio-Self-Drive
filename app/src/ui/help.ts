import { el } from "./dom";

const KEYS: [string, string][] = [
  ["Click / right-click", "Set the target; the driver drives there (click again to retarget)"],
  ["W / Up", "Accelerate (any drive key overrides the driver while held)"],
  ["S / Down", "Brake, then reverse"],
  ["A D / Left Right", "Steer"],
  ["Driver (panel)", "Policy server, A* baseline, or manual only"],
  ["R", "New random start pose (clears the target)"],
  ["Mouse wheel", "Zoom"],
  ["F", "Toggle free camera (drag to pan)"],
  ["Minimap click", "Jump camera (switches to free camera)"],
  ["P", "Pause / resume"],
  [".", "Step one decision (while paused)"],
  ["1 2 3 4 5", "Speed 1x 2x 4x 8x max"],
  ["Map picker", "Load a Factorio map from the pool (panel, right)"],
  ["Load map file", "Load a local .fsmap file"],
  ["T", "Game graphics / debug palette view"],
  ["B", "Collision boxes"],
  ["G", "Grid lines"],
  ["O", "Observation overlay"],
  ["H / Esc", "Toggle this help"],
];

export function buildHelp(root: HTMLElement): void {
  root.append(
    el("div", { className: "help-card" }, [
      el("h3", { textContent: "Controls" }),
      el(
        "table",
        {},
        KEYS.map(([k, d]) => el("tr", {}, [el("td", {}, [el("kbd", { textContent: k })]), el("td", { textContent: d })])),
      ),
    ]),
  );
  root.addEventListener("click", () => (root.hidden = true));
}
