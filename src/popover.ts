import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { mark } from "./marks";
import { COUNTED, elapsed, isLive, SessionEvent, State, STATE_TEXT } from "./session";

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const pills = el("pills");
const list = el("list");

const labels: Record<string, string> = {};
const agent = new URLSearchParams(location.search).get("agent") ?? "";

let sessions: SessionEvent[] = [];
let filter: State | null = null;
let drawn = "";
let sized = 0;
let clock: number | undefined;

const mine = () => sessions.filter((session) => session.agent === agent);

const report = (error: unknown) => console.error("ledge popover:", error);

function label(id: string) {
  return labels[id] ?? id;
}

function pill(state: State, total: number): HTMLElement {
  const button = document.createElement("button");
  button.className = filter === state ? "selected" : "";
  button.innerHTML = `${STATE_TEXT[state]} <b></b>`;
  button.querySelector("b")!.textContent = `${total}`;
  button.onclick = () => {
    filter = filter === state ? null : state;
    render(true);
  };
  return button;
}

function sessionRow(session: SessionEvent): HTMLElement {
  const row = document.createElement("div");
  row.className = "session";
  row.title = session.cwd ? `Open ${session.project_name} in Finder` : "";
  row.innerHTML = `
    <span class="session-ring ${session.state}"></span>
    <span>
      <div class="project"></div>
      <div class="state"></div>
    </span>
    <span class="elapsed"></span>`;

  row.querySelector(".session-ring")!.append(mark(session.agent, label(session.agent)));
  row.querySelector(".project")!.textContent = session.project_name;
  row.querySelector(".state")!.textContent =
    `${label(session.agent)} · ${STATE_TEXT[session.state]}`;
  row.querySelector(".elapsed")!.textContent = elapsed(session);

  if (session.cwd) {
    row.onclick = () =>
      invoke("open_project", { sessionId: session.session_id }).catch(() => {
        row.title = "That directory is gone";
      });
  }
  return row;
}

function signature(list: SessionEvent[]) {
  return list.map((s) => `${s.session_id}:${s.state}:${s.started_at}`).join("|");
}

function render(force = false) {
  const all = mine();
  if (filter && !all.some((session) => session.state === filter)) filter = null;

  const stamp = `${filter}|${signature(all)}|${all.filter((s) => isLive(s.state)).map(elapsed).join()}`;
  if (!force && stamp === drawn) return;
  drawn = stamp;

  const tallies = COUNTED.map(
    (state) => [state, all.filter((session) => session.state === state).length] as const,
  );

  pills.replaceChildren(
    ...tallies.filter(([, n]) => n > 0).map(([state, n]) => pill(state, n)),
  );

  const shown = filter ? all.filter((session) => session.state === filter) : all;
  if (!shown.length) {
    list.innerHTML = `<p class="empty">Nothing running</p>`;
  } else {
    list.replaceChildren(...shown.map(sessionRow));
  }

  resize();
  syncClock(all.some((session) => isLive(session.state)));
}

function syncClock(live: boolean) {
  if (live && clock === undefined) {
    clock = window.setInterval(render, 1000);
  } else if (!live && clock !== undefined) {
    clearInterval(clock);
    clock = undefined;
  }
}

function resize() {
  const card = document.querySelector(".popover") as HTMLElement;
  const height = Math.ceil(card.getBoundingClientRect().height);
  if (height === sized) return;

  sized = height;
  invoke("size_popover", { height }).catch(report);
}

listen<SessionEvent[]>("sessions", (event) => {
  sessions = event.payload;
  render();
});

let held = false;
getCurrentWindow().onFocusChanged(({ payload: focused }) => {
  if (focused) held = true;
  else if (held) invoke("close_popover").catch(report);
});

sessions = await invoke<SessionEvent[]>("get_sessions");
Object.assign(labels, await invoke<Record<string, string>>("agent_labels"));
render(true);
