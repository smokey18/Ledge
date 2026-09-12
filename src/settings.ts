import { invoke } from "@tauri-apps/api/core";

interface Integration {
  agent: string;
  label: string;
  available: boolean;
}

interface Prefs {
  autostart: boolean;
}

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const agents = el("agents");
const autostart = el<HTMLInputElement>("autostart");

function card(integration: Integration): HTMLElement {
  const element = document.createElement("div");
  element.className = "agent";

  const mark = document.createElement("span");
  mark.className = `agent-mark ${integration.agent}`;
  mark.textContent = integration.agent === "claude" ? "C" : ">_";
  mark.setAttribute("aria-hidden", "true");

  const body = document.createElement("div");
  body.className = "agent-body";

  const head = document.createElement("div");
  head.className = "agent-head";
  head.innerHTML = `<span class="agent-name"></span><span class="status"></span>`;
  head.querySelector(".agent-name")!.textContent = integration.label;

  const status = head.querySelector(".status") as HTMLElement;
  status.textContent = integration.available ? "Connected" : "Not installed";
  status.classList.toggle("on", integration.available);

  body.append(head);
  element.append(mark, body);

  return element;
}

async function render() {
  const [list, prefs] = await Promise.all([
    invoke<Integration[]>("integrations"),
    invoke<Prefs>("get_prefs"),
  ]);

  autostart.checked = prefs.autostart;

  const wrapper = document.createElement("div");
  wrapper.className = "card";
  wrapper.append(...list.map(card));
  agents.replaceChildren(wrapper);
}

autostart.addEventListener("change", () =>
  invoke("set_autostart", { enabled: autostart.checked }).catch(() => {
    autostart.checked = !autostart.checked;
  }),
);

render();
