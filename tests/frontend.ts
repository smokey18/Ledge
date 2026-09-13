import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { byAgent, elapsed, worstOf } from "../src/session";

const view = new URLSearchParams(location.search).get("view") ?? "popover";
const page = view === "main" ? "index" : view;

if (!["index", "popover", "settings"].includes(page)) {
  throw new Error("Unknown test page");
}

const html = new DOMParser().parseFromString(
  await (await fetch(`/${page}.html`)).text(),
  "text/html",
);

html.querySelectorAll("script").forEach((script) => script.remove());
document.head.append(...html.querySelectorAll('link[rel="stylesheet"]'));
document.body.replaceChildren(...html.body.childNodes);
document.documentElement.style.width =
  view === "main" ? "76px" : view === "popover" ? "268px" : "460px";
document.documentElement.style.height = view === "main" ? "238px" : "560px";

const result = document.createElement("pre");
result.id = "result";
result.style.cssText =
  "position:fixed;left:490px;top:20px;white-space:pre-wrap;color:green;width:380px";
document.body.append(result);

const assert = (condition: unknown, message: string) => {
  if (!condition) {
    throw new Error(message);
  }
};
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const sample = {
  session_id: "one",
  agent: "codex",
  project_name: "Ledge",
  title: null,
  cwd: "/tmp/Ledge",
  state: "working" as const,
  started_at: Date.now() - 1000,
  updated_at: Date.now(),
};
const calls: string[] = [];

mockWindows(view === "popover" ? "popover-codex" : view);
mockIPC(
  async (command) => {
    calls.push(command);

    if (command === "agent_labels") {
      return { codex: "Codex", claude: "Claude Code" };
    }
    if (command === "get_sessions") {
      await emit("sessions", [{ ...sample, title: "Fresh title" }]);
      return [sample];
    }
    if (command === "plugin:window|is_visible") {
      return true;
    }
    if (command === "get_prefs") {
      return { autostart: false };
    }
    if (command === "integrations") {
      return [{ agent: "codex", label: "Codex", available: true }];
    }
    if (command === "set_autostart") {
      throw new Error("simulated failure");
    }
  },
  { shouldMockEvents: true },
);

try {
  assert(
    byAgent([sample, sample]).get("codex")?.length === 2,
    "grouping lost sessions",
  );
  assert(
    worstOf([{ ...sample, state: "waiting" }, sample]) === "waiting",
    "wrong severity",
  );
  assert(
    elapsed({
      ...sample,
      state: "completed",
      started_at: 0,
      updated_at: 61000,
    }) === "1m 01s",
    "completed clock changed",
  );

  if (view === "main") {
    await import("../src/main");
    await wait(1100);

    assert(
      calls.filter((call) => call === "get_sessions").length === 1,
      "rail still polls",
    );
    assert(
      document.querySelector(".node")?.getAttribute("aria-label") ===
        "Codex, Running",
      "rail not rendered",
    );

    await emit("sessions", [{ ...sample, state: "waiting" }]);

    assert(document.querySelector(".node.waiting"), "rail missed state event");
    assert(
      calls.filter((call) => call === "set_agent_count").length === 1,
      "rail resized for unchanged count",
    );
  } else if (view === "popover") {
    await import("../src/popover");

    const row = document.querySelector(".session");
    assert(row?.tagName === "BUTTON", "session is not keyboard accessible");
    assert(
      document.querySelector(".project")?.textContent === "Fresh title",
      "initial snapshot replaced newer event",
    );

    const before = document.querySelector(".elapsed")?.textContent;
    await wait(1100);

    assert(row === document.querySelector(".session"), "clock replaced the row");
    assert(
      document.querySelector(".elapsed")?.textContent !== before,
      "live clock did not tick",
    );

    await emit("popover-visibility", false);
    const hidden = document.querySelector(".elapsed")?.textContent;
    await wait(1100);

    assert(
      document.querySelector(".elapsed")?.textContent === hidden,
      "hidden popover still ticks",
    );
    assert(
      calls.filter((call) => call === "get_sessions").length === 1,
      "popover still polls",
    );

    await emit("sessions", [{ ...sample, title: "Changed title" }]);
    assert(
      document.querySelector(".project")?.textContent === "Changed title",
      "title-only update missing",
    );
    await emit("sessions", [{ ...sample, state: "completed" }]);
  } else {
    await import("../src/settings");
    await wait(50);

    const checkbox = document.querySelector<HTMLInputElement>("#autostart")!;
    assert(!checkbox.disabled, "settings did not load");

    checkbox.click();
    assert(checkbox.disabled, "toggle allows concurrent writes");
    await wait(50);

    assert(
      !checkbox.checked && !checkbox.disabled,
      "failed toggle did not roll back",
    );
    assert(
      document.querySelector("#message")?.textContent?.includes("simulated failure"),
      "failure is invisible",
    );
  }

  result.textContent = `PASS: ${view}\nNo snapshot polling; state, clock and error checks passed.`;
} catch (error) {
  result.style.color = "red";
  result.textContent = `FAIL: ${view}\n${String(error)}`;
  throw error;
}
