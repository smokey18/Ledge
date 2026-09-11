import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type State = "working" | "waiting" | "completed" | "failed" | "idle";

interface SessionEvent {
  session_id: string;
  agent: string;
  project_name: string;
  cwd: string;
  state: State;
  started_at: number;
  updated_at: number;
}




const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const dot = el("dot");
const headline = el("headline");
const expandButton = el<HTMLButtonElement>("expand");
const settingsButton = el<HTMLButtonElement>("settings");
const track = el("track");
const sessionsPanel = el("sessions");

const STATE_TEXT: Record<State, string> = {
  working: "working",
  waiting: "needs you",
  completed: "finished",
  failed: "failed",
  idle: "idle",
};

const SEVERITY: State[] = ["waiting", "failed", "working", "completed", "idle"];

const labels: Record<string, string> = {};

let sessions: SessionEvent[] = [];
let expanded = false;
let clock: number | undefined;
const collapsed = new Set<string>();

function elapsed(since: number): string {
  const total = Math.max(0, Math.floor((Date.now() - since) / 1000));
  if (total < 60) return `${total}s`;

  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

function headlineFor(lead: State, group: SessionEvent[]): string {
  if (!sessions.length) return "No sessions";
  const only = group.length === 1 ? group[0].project_name : "";

  switch (lead) {
    case "waiting":
      return only ? `${only} needs you` : `${group.length} need you`;
    case "working":
      return only ? `${only} · working` : `${group.length} working`;
    case "failed":
      return only ? `${only} · failed` : `${group.length} failed`;
    case "completed":
      return only ? `${only} · finished` : `${group.length} finished`;
    default:
      return "Nothing running";
  }
}

function sessionRow(session: SessionEvent): HTMLElement {
  const row = document.createElement("div");
  row.className = "session";
  row.title = session.cwd ? `Open ${session.project_name} in Finder` : "";
  row.innerHTML = `
    <span class="dot ${session.state}"></span>
    <span class="body">
      <div class="project"></div>
      <div class="state ${session.state}"></div>
    </span>
    <span class="elapsed"></span>`;

  row.querySelector(".project")!.textContent = session.project_name;
  row.querySelector(".state")!.textContent = STATE_TEXT[session.state];
  row.querySelector(".elapsed")!.textContent = elapsed(session.started_at);

  if (session.cwd) {
    row.onclick = () =>
      invoke("open_project", { sessionId: session.session_id }).catch(() => {
        row.title = "That directory is gone";
      });
  }
  return row;
}

function agentGroup(agent: string, label: string, group: SessionEvent[]): HTMLElement {
  const worst = SEVERITY.find((state) => group.some((s) => s.state === state)) ?? "idle";

  const details = document.createElement("details");
  details.className = "group";
  details.open = !collapsed.has(agent);
  details.ontoggle = () => (details.open ? collapsed.delete(agent) : collapsed.add(agent));

  const summary = document.createElement("summary");
  summary.innerHTML = `
    <span class="dot ${worst}"></span>
    <span class="agent-name"></span>
    <span class="tally"></span>
    <span class="chevron">›</span>`;
  summary.querySelector(".agent-name")!.textContent = label;
  summary.querySelector(".tally")!.textContent = `${group.length}`;

  details.append(summary, ...group.map(sessionRow));
  return details;
}

function renderSessions() {
  if (!sessions.length) {
    sessionsPanel.innerHTML = `<p class="empty">Nothing running</p>`;
    return;
  }

  const byAgent = new Map<string, SessionEvent[]>();
  for (const session of sessions) {
    byAgent.set(session.agent, [...(byAgent.get(session.agent) ?? []), session]);
  }

  sessionsPanel.replaceChildren(
    ...[...byAgent].map(([agent, group]) => agentGroup(agent, labels[agent] ?? agent, group)),
  );
}

function render() {
  const working = sessions.filter((session) => session.state === "working");
  const latest = sessions[0];
  const lead = latest?.state ?? "idle";

  dot.className = `dot ${lead}`;
  headline.textContent = headlineFor(lead, latest ? [latest] : []);

  track.classList.toggle("active", working.length > 0);

  if (expanded) renderSessions();
  syncClock(working.length > 0);
}

/** The clock exists only while work is live, so a static widget never repaints. */
function syncClock(working: boolean) {
  const wanted = working && expanded;

  if (wanted && clock === undefined) {
    clock = window.setInterval(renderSessions, 1000);
  } else if (!wanted && clock !== undefined) {
    clearInterval(clock);
    clock = undefined;
  }
}





function setExpanded(next: boolean) {
  expanded = next;
  document.body.classList.toggle("expanded", expanded);
  expandButton.setAttribute("aria-expanded", `${expanded}`);
  expandButton.setAttribute("aria-label", expanded ? "Collapse" : "Expand");
  render();
  // Resizing the window is Rust's job, but drawing must not wait on it.
  invoke("set_expanded", { expanded }).catch(() => {});
}

expandButton.addEventListener("click", () => setExpanded(!expanded));

// The window grows after the list is drawn, and a transparent WebView does not
// reliably repaint the area that reveals.
window.addEventListener("resize", () => {
  if (expanded) renderSessions();
});

settingsButton.addEventListener("click", () => invoke("open_settings"));




listen<SessionEvent[]>("sessions", (event) => {
  sessions = event.payload;
  render();
});

sessions = await invoke<SessionEvent[]>("get_sessions");
Object.assign(labels, await invoke<Record<string, string>>("agent_labels"));
setExpanded((await invoke<{ expanded: boolean }>("get_prefs")).expanded);
