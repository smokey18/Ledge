import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { mark } from "./marks";
import { byAgent, SessionEvent, STATE_TEXT, worstOf } from "./session";

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const orb = el<HTMLButtonElement>("orb");
const nodes = el("nodes");
const settingsButton = el<HTMLButtonElement>("settings");

const labels: Record<string, string> = {};

const report = (error: unknown) => console.error("ledge rail:", error);

let sessions: SessionEvent[] = [];
let openAgent: string | null = null;
let drawn = "";
let agentCount = -1;
let received = false;

function label(agent: string) {
  return labels[agent] ?? agent;
}

function agentNode(agent: string, group: SessionEvent[]): HTMLElement {
  const node = document.createElement("button");
  const state = worstOf(group);
  node.className = `node ${state}`;
  node.setAttribute("aria-label", `${label(agent)}, ${STATE_TEXT[state]}`);
  node.append(mark(agent, label(agent)));

  if (group.length > 1) {
    const tally = document.createElement("span");
    tally.className = "tally";
    tally.textContent = `${group.length}`;
    node.append(tally);
  }

  node.onclick = () => {
    openAgent = agent;
    invoke("open_popover", { agent, anchor: node.offsetTop + node.offsetHeight / 2 }).catch(
      (error) => {
        openAgent = null;
        report(error);
      },
    );
  };
  return node;
}

function closePopover() {
  openAgent = null;
  invoke("close_popover").catch(report);
}

function render(force = false) {
  const groups = [...byAgent(sessions)].sort(([a], [b]) => a.localeCompare(b));
  const stamp = JSON.stringify(groups.map(([agent, group]) => [agent, group.length, worstOf(group)]));
  if (!force && stamp === drawn) return;
  drawn = stamp;

  orb.className = `orb ${worstOf(sessions)}`;
  orb.title = sessions.length
    ? `${sessions.length} session${sessions.length === 1 ? "" : "s"}`
    : "No sessions";

  nodes.replaceChildren(...groups.map(([agent, group]) => agentNode(agent, group)));

  if (groups.length !== agentCount) {
    agentCount = groups.length;
    invoke("set_agent_count", { count: agentCount }).catch(report);
  }

  if (openAgent && !groups.some(([agent]) => agent === openAgent)) closePopover();
}

orb.addEventListener("click", () => (openAgent ? closePopover() : undefined));

settingsButton.addEventListener("click", (event) => {
  event.stopPropagation();
  invoke("open_settings").catch(report);
});

Object.assign(labels, await invoke<Record<string, string>>("agent_labels"));
await listen<SessionEvent[]>("sessions", (event) => {
  received = true;
  sessions = event.payload;
  render();
});
await listen("popover-closed", () => (openAgent = null));

const initial = await invoke<SessionEvent[]>("get_sessions");
if (!received) sessions = initial;
render(true);
