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

function label(agent: string) {
  return labels[agent] ?? agent;
}

function agentNode(agent: string, group: SessionEvent[]): HTMLElement {
  const node = document.createElement("button");
  node.className = `node ${worstOf(group)}`;
  node.setAttribute("aria-label", `${label(agent)}, ${STATE_TEXT[worstOf(group)]}`);
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

function signature(list: SessionEvent[]) {
  return list.map((s) => `${s.agent}:${s.session_id}:${s.state}`).join("|");
}

function render(force = false) {
  const stamp = signature(sessions);
  if (!force && stamp === drawn) return;
  drawn = stamp;

  const groups = [...byAgent(sessions)];

  orb.className = `orb ${worstOf(sessions)}`;
  orb.title = sessions.length
    ? `${sessions.length} session${sessions.length === 1 ? "" : "s"}`
    : "No sessions";

  nodes.replaceChildren(...groups.map(([agent, group]) => agentNode(agent, group)));

  invoke("set_agent_count", { count: groups.length }).catch(report);

  if (openAgent && !groups.some(([agent]) => agent === openAgent)) closePopover();
}

orb.addEventListener("click", () => (openAgent ? closePopover() : undefined));

settingsButton.addEventListener("click", (event) => {
  event.stopPropagation();
  invoke("open_settings");
});

listen<SessionEvent[]>("sessions", (event) => {
  sessions = event.payload;
  render();
});

setInterval(async () => {
  sessions = await invoke<SessionEvent[]>("get_sessions");
  render();
}, 1000);

listen("popover-closed", () => (openAgent = null));

sessions = await invoke<SessionEvent[]>("get_sessions");
Object.assign(labels, await invoke<Record<string, string>>("agent_labels"));
render(true);
