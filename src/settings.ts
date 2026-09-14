import { invoke } from "@tauri-apps/api/core";
import { mark } from "./marks";

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
const message = el("message");
autostart.disabled = true;

function card(integration: Integration): HTMLElement {
  const element = document.createElement("div");
  element.className = "agent";

  const icon = document.createElement("span");
  icon.className = "agent-mark";
  icon.setAttribute("aria-hidden", "true");
  icon.append(mark(integration.agent, integration.label));

  const body = document.createElement("div");
  body.className = "agent-body";

  const head = document.createElement("div");
  head.className = "agent-head";
  head.innerHTML = `<span class="agent-name"></span><span class="status"></span>`;
  head.querySelector(".agent-name")!.textContent = integration.label;

  const status = head.querySelector(".status") as HTMLElement;
  status.textContent = integration.available ? "Connected" : "Disconnected";
  status.classList.toggle("on", integration.available);

  body.append(head);
  element.append(icon, body);

  return element;
}

async function render() {
  const [list, prefs] = await Promise.all([
    invoke<Integration[]>("integrations"),
    invoke<Prefs>("get_prefs"),
  ]);

  autostart.checked = prefs.autostart;
  autostart.disabled = false;

  const wrapper = document.createElement("div");
  wrapper.className = "card";
  wrapper.append(...list.map(card));
  agents.replaceChildren(wrapper);
}

autostart.addEventListener("change", async () => {
  const enabled = autostart.checked;
  autostart.disabled = true;
  message.textContent = "";
  try {
    await invoke("set_autostart", { enabled });
  } catch (error) {
    autostart.checked = !enabled;
    message.textContent = `Could not change Open at login: ${String(error)}`;
  } finally {
    autostart.disabled = false;
  }
});

render().catch((error) => {
  message.textContent = `Could not load settings: ${String(error)}`;
});
