import { POLICY_URL, SERVE_COMMAND, type DriverKind, type LiveSession } from "../rl/controller";
import type { ConnectionState } from "../rl/policyClient";
import { el } from "./dom";

const DRIVERS: [DriverKind, string][] = [
  ["policy", "Policy (server)"],
  ["baseline", "A* baseline"],
  ["manual", "Manual only"],
];

// Driver selector, the policy server connection and the session's status line.
export class DriverControls {
  private status = el("div", { className: "map-status session-status" });
  private connStatus = el("div", { className: "map-status" });
  private helloInfo = el("div", { className: "map-status" });
  private serveHint = el("div", { className: "map-status" }, [
    "Start the server: ",
    el("code", { textContent: SERVE_COMMAND }),
  ]);
  private policyBox: HTMLElement;

  constructor(
    root: HTMLElement,
    private live: LiveSession,
  ) {
    const driver = el("select");
    driver.replaceChildren(...DRIVERS.map(([value, label]) => el("option", { value, textContent: label })));
    driver.value = live.driverKind;
    driver.addEventListener("change", () => {
      live.setDriver(driver.value as DriverKind);
      this.policyBox.hidden = live.driverKind !== "policy";
      driver.blur();
    });
    const reconnect = el("button", {
      textContent: "Reconnect",
      onclick: (e: MouseEvent) => {
        live.connect();
        (e.currentTarget as HTMLElement).blur();
      },
    });
    this.policyBox = el("div", {}, [
      el("div", { className: "map-status", textContent: POLICY_URL }),
      this.connStatus,
      this.helloInfo,
      this.serveHint,
      el("div", { className: "buttons" }, [reconnect]),
    ]);
    root.append(
      el("h3", { textContent: "Driver" }),
      el("label", { className: "field" }, [el("span", { textContent: "Drives to target" }), driver]),
      this.policyBox,
      this.status,
    );
  }

  setStatus(text: string): void {
    this.status.textContent = text;
  }

  setConnection(state: ConnectionState, message: string): void {
    this.connStatus.textContent = message;
    this.connStatus.className = `map-status conn-${state}`;
    this.serveHint.hidden = state === "ready" || state === "connecting";
    const hello = this.live.client.hello;
    this.helloInfo.textContent = hello
      ? `mode ${hello.mode}, decision_ticks ${hello.decision_ticks}, model ${JSON.stringify(hello.model)}` +
        (Object.keys(hello.mode_config).length ? `, ${JSON.stringify(hello.mode_config)}` : "")
      : "";
  }
}
