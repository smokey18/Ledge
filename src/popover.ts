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
let held = false;
let received = false;
let visible = false;
let timer: ReturnType<typeof setTimeout> | undefined;
let shown: SessionEvent[] = [];

const mine = () => sessions.filter((session) => session.agent === agent);

const report = (error: unknown) => console.error("ledge popover:", error);

function label(id: string) {
  return labels[id] ?? id;
}

function pill(state: State, total: number): HTMLElement {
  const button = document.createElement("button");
  button.className = filter === state ? "selected" : "";
  button.setAttribute("aria-pressed", String(filter === state));
  button.innerHTML = `${STATE_TEXT[state]} <b></b>`;
  button.querySelector("b")!.textContent = `${total}`;
  button.onclick = () => {
    filter = filter === state ? null : state;
    render(true);
  };
  return button;
}

function sessionRow(session: SessionEvent): HTMLElement {
  const row = document.createElement("button");
  row.disabled = !session.cwd;
  row.className = "session";
  row.title = session.cwd ? `Open ${session.project_name} in Finder` : "";
  row.innerHTML = `
    <span class="session-ring ${session.state}"></span>
    <span>
      <span class="project"></span>
      <span class="state"></span>
    </span>
    <span class="elapsed"></span>`;

  row.querySelector(".session-ring")!.append(mark(session.agent, label(session.agent)));
  row.querySelector(".project")!.textContent = session.title ?? session.project_name;
  row.querySelector(".state")!.textContent = session.title
    ? `${session.project_name} · ${STATE_TEXT[session.state]}`
    : `${label(session.agent)} · ${STATE_TEXT[session.state]}`;
  row.querySelector(".elapsed")!.textContent = elapsed(session);

  if (session.cwd) {
    row.onclick = () =>
      invoke("open_project", { sessionId: session.session_id }).catch(() => {
        row.title = "That directory is gone";
      });
  }
  return row;
}

function tick() {
  clearTimeout(timer);
  timer = undefined;
  if (!visible) return;
  list.querySelectorAll(".elapsed").forEach((element, index) => {
    const text = elapsed(shown[index]);
    if (element.textContent !== text) element.textContent = text;
  });
  if (shown.some((session) => isLive(session.state))) timer = setTimeout(tick, 1000);
}

function render(force = false) {
  const all = mine();
  if (filter && !all.some((session) => session.state === filter)) filter = null;

  const stamp = JSON.stringify([filter, all]);
  if (!force && stamp === drawn) return;
  drawn = stamp;

  const tallies = COUNTED.map(
    (state) => [state, all.filter((session) => session.state === state).length] as const,
  );

  pills.replaceChildren(
    ...tallies.filter(([, n]) => n > 0).map(([state, n]) => pill(state, n)),
  );

  shown = filter ? all.filter((session) => session.state === filter) : all;
  if (!shown.length) {
    list.innerHTML = `<p class="empty">Nothing running</p>`;
  } else {
    list.replaceChildren(...shown.map(sessionRow));
  }
  tick();
}

const card = document.querySelector(".popover") as HTMLElement;

function resize() {
  const height = Math.ceil(card.getBoundingClientRect().height);
  if (height === sized || height === 0) return;

  sized = height;
  invoke("size_popover", { agent, height }).catch(report);
}

new ResizeObserver(resize).observe(card);

Object.assign(labels, await invoke<Record<string, string>>("agent_labels"));
await listen<SessionEvent[]>("sessions", (event) => {
  received = true;
  sessions = event.payload;
  render();
});
await getCurrentWindow().listen<boolean>("popover-visibility", ({ payload }) => {
  visible = payload;
  if (visible) {
    sized = 0;
    resize();
  }
  tick();
});
await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
  if (focused) held = true;
  else if (held) invoke("close_popover", { agent }).catch(report);
});

const initial = await invoke<SessionEvent[]>("get_sessions");
if (!received) sessions = initial;
visible = await getCurrentWindow().isVisible();
held = await getCurrentWindow().isFocused();
render(true);
