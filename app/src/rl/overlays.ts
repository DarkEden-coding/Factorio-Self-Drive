import type { Settings } from "../settings";
import type { EpisodeSession } from "./episode";

const FIELD_ALPHA = 0.45;

// RL overlays in world coordinates, drawn over both graphics modes. The context already
// has the world transform (1 unit = 1 tile).
export function drawRlOverlays(ctx: CanvasRenderingContext2D, session: EpisodeSession, settings: Settings, zoom: number): void {
  if (!session.info) return;
  if (settings.showField) {
    const field = session.fieldImage();
    if (field) {
      const { x_min, y_min, width, height } = field.info;
      ctx.globalAlpha = FIELD_ALPHA;
      ctx.imageSmoothingEnabled = false;
      ctx.drawImage(field.canvas, x_min, y_min, width, height);
      ctx.globalAlpha = 1;
    }
  }
  if (settings.showBaseline) {
    const path = session.baselinePath();
    if (path) polyline(ctx, path, 0, path.length / 2, "rgba(255, 255, 255, 0.75)", 1.5 / zoom, [4 / zoom, 4 / zoom]);
  }
  if (settings.showTrail) {
    polyline(ctx, session.trail, 0, session.trail.length / 2, "rgba(255, 120, 60, 0.85)", 2 / zoom);
  }
  if (settings.showPlan && session.plan) drawPlan(ctx, session, zoom);
}

// The plan as a polyline with points; its executed part is the trail since it was made.
function drawPlan(ctx: CanvasRenderingContext2D, session: EpisodeSession, zoom: number): void {
  const plan = session.plan!;
  const color = plan.kind === "waypoints" ? "#7cf2ff" : "#c792ff";
  const n = plan.points.length / 2;
  // Plans start at the vehicle position where they were made.
  const sx = session.trail[plan.startDecision * 2];
  const sy = session.trail[plan.startDecision * 2 + 1];
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.5 / zoom;
  ctx.beginPath();
  if (sx !== undefined) ctx.moveTo(sx, sy);
  for (let i = 0; i < n; i++) ctx.lineTo(plan.points[i * 2], plan.points[i * 2 + 1]);
  ctx.stroke();

  const executed = session.decisions - plan.startDecision;
  for (let i = 0; i < n; i++) {
    // Predicted points are one per decision, so the first `executed` have been driven.
    const done = plan.kind === "predicted" && i < executed;
    ctx.beginPath();
    ctx.arc(plan.points[i * 2], plan.points[i * 2 + 1], (done ? 3 : 2.5) / zoom, 0, Math.PI * 2);
    ctx.fillStyle = done ? "#ffffff" : color;
    ctx.fill();
  }
  polyline(ctx, session.trail, plan.startDecision, session.trail.length / 2, "#ffffff", 3 / zoom);
}

function polyline(
  ctx: CanvasRenderingContext2D,
  points: ArrayLike<number>,
  from: number,
  to: number,
  color: string,
  width: number,
  dash: number[] = [],
): void {
  if (to - from < 2) return;
  ctx.strokeStyle = color;
  ctx.lineWidth = width;
  ctx.lineJoin = "round";
  ctx.setLineDash(dash);
  ctx.beginPath();
  ctx.moveTo(points[from * 2], points[from * 2 + 1]);
  for (let i = from + 1; i < to; i++) ctx.lineTo(points[i * 2], points[i * 2 + 1]);
  ctx.stroke();
  ctx.setLineDash([]);
}
