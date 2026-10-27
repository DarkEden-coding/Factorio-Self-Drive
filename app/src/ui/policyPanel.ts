import type { EpisodeSession } from "../rl/episode";
import { ACTION_COUNT, actionLabel } from "../rl/types";
import { el } from "./dom";

const UPDATE_INTERVAL_MS = 100;
// Grid rows are throttle forward, coast, reverse; columns steer left, straight, right.
const ROW_THROTTLE = [2, 1, 0];

// Model outputs and episode bookkeeping: action probabilities, value, last action,
// decision count, reward totals by term and path distance.
export class PolicyPanel {
  private cells: HTMLElement[] = [];
  private lines = el("div", { className: "policy-lines" });
  private rewards = el("table", { className: "reward-table" });
  private lastUpdate = 0;

  constructor(
    root: HTMLElement,
    private session: EpisodeSession,
  ) {
    const grid = el("div", { className: "prob-grid" });
    for (const throttle of ROW_THROTTLE) {
      for (let steer = 0; steer < 3; steer++) {
        const cell = el("div", { className: "prob-cell", title: actionLabel(throttle * 3 + steer) });
        this.cells[throttle * 3 + steer] = cell;
        grid.append(cell);
      }
    }
    root.append(
      el("h3", { textContent: "Policy" }),
      el("div", { className: "policy-top" }, [
        el("div", {}, [grid, el("small", { className: "muted", textContent: "rows W/coast/S, cols A/-/D" })]),
        this.lines,
      ]),
      el("h4", { textContent: "Episode reward" }),
      this.rewards,
    );
  }

  update(now: number): void {
    if (now - this.lastUpdate < UPDATE_INTERVAL_MS) return;
    this.lastUpdate = now;
    const s = this.session;
    const probs = s.policy?.probs ?? null;
    const action = s.last?.action ?? -1;
    for (let a = 0; a < ACTION_COUNT; a++) {
      const p = probs?.[a];
      const cell = this.cells[a];
      cell.textContent = p === undefined ? "-" : p.toFixed(2);
      cell.style.background = p === undefined ? "" : `rgba(74, 163, 223, ${Math.min(1, p).toFixed(3)})`;
      cell.classList.toggle("chosen", a === action);
    }

    const state = s.state;
    const value = s.policy?.value;
    const rows: [string, string][] = [
      ["value", value === null || value === undefined ? "-" : value.toFixed(3)],
      ["action", action >= 0 ? `${action} ${actionLabel(action)}` : "-"],
      ["decisions", state ? String(state.decisions) : "-"],
      ["status", state ? `${state.status}  ${state.ticks}/${state.time_limit_ticks} t` : "no episode"],
      ["path", state ? `${state.path_distance.toFixed(1)} / ${state.optimal_distance.toFixed(1)} tiles` : "-"],
      ["hits", state ? `${state.collisions}  dmg ${state.damage.toFixed(0)}` : "-"],
    ];
    this.lines.replaceChildren(
      ...rows.map(([k, v]) => el("div", {}, [el("span", { className: "k", textContent: k }), v])),
    );

    if (!state) {
      this.rewards.replaceChildren();
      return;
    }
    const terms = Object.entries(state.reward);
    this.rewards.replaceChildren(
      el("tr", {}, ["term", "total", "last"].map((h) => el("th", { textContent: h }))),
      ...[...terms, ["total", state.reward_total] as [string, number]].map(([term, v]) =>
        el("tr", { className: term === "total" ? "total" : "" }, [
          el("td", { textContent: term }),
          el("td", { textContent: v.toFixed(3) }),
          el("td", { textContent: s.last && term in s.last.terms ? s.last.terms[term].toFixed(3) : "" }),
        ]),
      ),
    );
  }
}
