import type { EpisodeStatus, TargetState, VehicleState } from "../sim/types";
import { el } from "./dom";

export interface HudExtras {
  paused: boolean;
  speedLabel: string;
  freeCamera: boolean;
  perf: string;
  // Status of the current episode, if any.
  episodeStatus: EpisodeStatus | null;
  // Short-lived banner text that wins over the episode status (e.g. "UNREACHABLE").
  notice: string | null;
}

const EPISODE_BANNERS: Record<Exclude<EpisodeStatus, "running">, [string, "good" | "bad"]> = {
  success: ["REACHED", "good"],
  destroyed: ["DESTROYED - press R", "bad"],
  timeout: ["TIMEOUT - click to retry", "bad"],
  stalled: ["STALLED - click to retry", "bad"],
};

export class Hud {
  private lines = el("div", { className: "hud-lines" });
  private healthFill = el("div", { className: "bar-fill" });
  private healthText = el("span");
  private banner: HTMLElement;

  constructor(root: HTMLElement, banner: HTMLElement) {
    this.banner = banner;
    root.append(
      this.lines,
      el("div", { className: "bar" }, [this.healthFill, this.healthText]),
    );
  }

  update(v: VehicleState, target: TargetState | null, x: HudExtras): void {
    const kmh = v.speed * 216;
    const c = v.last_collision;
    const rows: [string, string][] = [
      ["vehicle", `${v.kind}  fuel ${v.fuel}`],
      ["speed", `${kmh.toFixed(1)} km/h${kmh < -0.05 ? " (reverse)" : ""}`],
      ["tick", `${v.tick}  (${(v.tick / 60).toFixed(1)} s)`],
      ["target", target ? `${target.distance.toFixed(1)} m` : "none"],
      ["pos", `${v.x.toFixed(1)}, ${v.y.toFixed(1)}`],
      ["sim", `${x.paused ? "PAUSED" : x.speedLabel}${x.freeCamera ? "  free cam" : ""}`],
      ["frame", x.perf],
      ["hit", c ? `t${c.tick} ${c.kind} #${c.entity_id} -${c.damage_taken.toFixed(1)} hp` : "none"],
    ];
    this.lines.replaceChildren(
      ...rows.map(([k, val]) => el("div", {}, [el("span", { className: "k", textContent: k }), val])),
    );

    const frac = v.max_health > 0 ? Math.max(0, v.health / v.max_health) : 0;
    this.healthFill.style.width = `${frac * 100}%`;
    this.healthFill.style.background = frac > 0.5 ? "#4cc16a" : frac > 0.25 ? "#e0b341" : "#d9473f";
    this.healthText.textContent = `${Math.round(v.health)} / ${Math.round(v.max_health)}`;

    if (x.notice) this.showBanner(x.notice, "bad");
    else if (x.episodeStatus && x.episodeStatus !== "running") this.showBanner(...EPISODE_BANNERS[x.episodeStatus]);
    else if (v.destroyed) this.showBanner("VEHICLE DESTROYED", "bad");
    else this.banner.hidden = true;
  }

  private showBanner(text: string, kind: "good" | "bad"): void {
    this.banner.hidden = false;
    this.banner.textContent = text;
    this.banner.className = kind;
  }
}
