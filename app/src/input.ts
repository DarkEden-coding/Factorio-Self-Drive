import type { Camera } from "./camera";
import type { DriveInput } from "./loop";

const FORWARD = ["KeyW", "ArrowUp"];
const BACK = ["KeyS", "ArrowDown"];
const LEFT = ["KeyA", "ArrowLeft"];
const RIGHT = ["KeyD", "ArrowRight"];
const DRIVE_KEYS = [...FORWARD, ...BACK, ...LEFT, ...RIGHT];

function isTyping(target: EventTarget | null): boolean {
  return target instanceof HTMLInputElement || target instanceof HTMLSelectElement || target instanceof HTMLTextAreaElement;
}

// Held-key driving state plus one-shot key presses for everything else.
export class KeyInput {
  private held = new Set<string>();

  constructor(onPress: (code: string) => void) {
    window.addEventListener("keydown", (e) => {
      if (isTyping(e.target) || e.ctrlKey || e.metaKey || e.altKey) return;
      if (DRIVE_KEYS.includes(e.code)) {
        e.preventDefault();
        this.held.add(e.code);
      } else if (!e.repeat) {
        onPress(e.code);
      }
    });
    window.addEventListener("keyup", (e) => this.held.delete(e.code));
    window.addEventListener("blur", () => this.held.clear());
  }

  read(): DriveInput {
    const any = (codes: string[]) => codes.some((c) => this.held.has(c));
    return {
      throttle: (any(FORWARD) ? 1 : 0) - (any(BACK) ? 1 : 0),
      steer: (any(RIGHT) ? 1 : 0) - (any(LEFT) ? 1 : 0),
    };
  }
}

// A press that moves less than this many pixels before release is a click, not a drag.
const CLICK_SLOP_PX = 5;

// Mouse handling on the world canvas: wheel zoom, left-drag pan (free camera only), and
// a left or right click to set the target.
export function bindWorldPointer(
  canvas: HTMLCanvasElement,
  camera: Camera,
  isFree: () => boolean,
  onSetTarget: (x: number, y: number) => void,
): void {
  let press: { x: number; y: number; moved: boolean } | null = null;
  let dragging = false;
  canvas.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const p = camera.toWorld(e.offsetX, e.offsetY);
    onSetTarget(p.x, p.y);
  });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    camera.zoomAt(e.offsetX, e.offsetY, Math.exp(-e.deltaY * 0.0015));
  }, { passive: false });
  canvas.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    press = { x: e.clientX, y: e.clientY, moved: false };
    dragging = isFree();
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!press) return;
    if (Math.hypot(e.clientX - press.x, e.clientY - press.y) > CLICK_SLOP_PX) press.moved = true;
    if (dragging) camera.panByPixels(e.movementX, e.movementY);
  });
  canvas.addEventListener("pointerup", (e) => {
    if (e.button === 0 && press && !press.moved) {
      const p = camera.toWorld(e.offsetX, e.offsetY);
      onSetTarget(p.x, p.y);
    }
    press = null;
    dragging = false;
  });
  canvas.addEventListener("pointercancel", () => {
    press = null;
    dragging = false;
  });
}
