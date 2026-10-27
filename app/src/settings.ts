export interface Settings {
  showBoxes: boolean;
  showGrid: boolean;
  showObs: boolean;
  freeCamera: boolean;
  // Game graphics (textures and sprites) instead of the debug palette view.
  gameGraphics: boolean;
  // RL episode overlays.
  showTrail: boolean;
  showField: boolean;
  showBaseline: boolean;
  showPlan: boolean;
}

export function defaultSettings(): Settings {
  return {
    showBoxes: false,
    showGrid: false,
    showObs: false,
    freeCamera: false,
    gameGraphics: false,
    showTrail: true,
    showField: false,
    showBaseline: false,
    showPlan: true,
  };
}
