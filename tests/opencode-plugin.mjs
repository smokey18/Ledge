import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const home = mkdtempSync(join(tmpdir(), "lg-"));
mkdirSync(join(home, ".ledge"));
process.env.HOME = home;

let received = [];
const server = createServer((socket) => {
  let data = "";
  socket.on("data", (chunk) => (data += chunk));
  socket.on("end", () => received.push(data));
});
await new Promise((resolve) => server.listen(join(home, ".ledge", "hook.sock"), resolve));

const settle = () => new Promise((resolve) => setTimeout(resolve, 300));
const collect = async () => {
  await settle();
  const messages = received.map((message) => {
    const split = message.indexOf("\n");
    assert.equal(message.slice(0, split), "opencode");
    assert.ok(message.length < 1024, `sent ${message.length} bytes`);
    return JSON.parse(message.slice(split + 1));
  });
  received = [];
  return messages;
};

const plugin = new URL("../src-tauri/src/opencode-plugin.js", import.meta.url);
const { default: definition } = await import(pathToFileURL(plugin.pathname).href);
assert.equal(definition.id, "ledge");
assert.equal(typeof definition.server, "function");
assert.equal(typeof definition.setup, "function");

const { event } = await definition.server({ directory: "/Users/x/v1" });
const v1 = (type, properties) => event({ event: { type, properties } });
await v1("session.created", { info: { id: "s1", title: "New session - 2026" } });
await v1("session.updated", { info: { id: "s1", title: "Fix login bug" } });
await v1("session.created", { info: { id: "c1", parentID: "s1", title: "Child" } });
await v1("session.status", { sessionID: "s1", status: { type: "busy" } });
await v1("session.status", { sessionID: "c1", status: { type: "busy" } });
await v1("session.status", { sessionID: "s1", status: { type: "idle" } });
await v1("message.updated", { sessionID: "s1" });
await v1("permission.asked", { sessionID: "s1", metadata: { diff: "x".repeat(200_000) } });
await v1("permission.replied", { sessionID: "s1", reply: "once" });
await v1("question.asked", { sessionID: "s1" });
await v1("question.rejected", { sessionID: "s1" });
await v1("session.error", { sessionID: "s1", error: { name: "MessageAbortedError" } });
await v1("session.error", { sessionID: "s1", error: { name: "ApiError" } });
await v1("session.idle", { sessionID: "s1" });

const fromV1 = await collect();
assert.deepEqual(
  fromV1.map((message) => message.state),
  ["working", "waiting", "working", "waiting", "working", "done", "failed", "done"],
);
for (const message of fromV1) {
  assert.equal(message.sessionID, "s1");
  assert.equal(message.directory, "/Users/x/v1");
  assert.equal(message.title, "Fix login bug");
}

const v2Events = [
  { type: "session.created", data: { sessionID: "s2", title: "New session - 2026" } },
  { type: "session.renamed", data: { sessionID: "s2", title: "Add dark mode" } },
  { type: "session.created", data: { sessionID: "c2", parentID: "s2", title: "Child" } },
  { type: "session.execution.started", data: { sessionID: "s2" }, location: { directory: "/Users/x/v2" } },
  { type: "session.execution.started", data: { sessionID: "c2" }, location: { directory: "/Users/x/v2" } },
  { type: "permission.asked", data: { sessionID: "s2", metadata: { diff: "x".repeat(200_000) } } },
  { type: "permission.replied", data: { sessionID: "s2", reply: "once" } },
  { type: "form.created", data: { sessionID: "s2" } },
  { type: "form.replied", data: { sessionID: "s2" } },
  { type: "session.tool.input.started", data: { sessionID: "s2", id: "t1", name: "bash" } },
  { type: "session.tool.called", data: { sessionID: "s2", id: "t1" } },
  { type: "session.tool.success", data: { sessionID: "s2", id: "t1" } },
  { type: "session.tool.input.started", data: { sessionID: "s2", id: "q1", name: "question" } },
  { type: "session.tool.called", data: { sessionID: "s2", id: "q1" } },
  { type: "session.tool.success", data: { sessionID: "s2", id: "q1" } },
  { type: "session.execution.failed", data: { sessionID: "s2", error: { type: "api", message: "x" } } },
  { type: "session.execution.interrupted", data: { sessionID: "s2", reason: "user" } },
  { type: "session.execution.succeeded", data: { sessionID: "s2" } },
];
let subscribed;
let stopped = false;
const cleanup = definition.setup({
  location: { directory: "/Users/x/v2" },
  event: {
    subscribe: (options) => {
      subscribed = options;
      return {
        [Symbol.asyncIterator]: () => {
          let index = 0;
          return {
            next: async () => (index < v2Events.length ? { value: v2Events[index++], done: false } : new Promise(() => {})),
            return: async () => {
              stopped = true;
              return { done: true };
            },
          };
        },
      };
    },
  },
});

const fromV2 = await collect();
assert.ok(subscribed.types.includes("session.execution.succeeded"));
assert.ok(subscribed.types.includes("form.created"));
assert.ok(subscribed.types.includes("session.tool.called"));
assert.deepEqual(
  fromV2.map((message) => message.state),
  ["working", "waiting", "working", "waiting", "working", "waiting", "working", "failed", "done", "done"],
);
for (const message of fromV2) {
  assert.equal(message.sessionID, "s2");
  assert.equal(message.directory, "/Users/x/v2");
  assert.equal(message.title, "Add dark mode");
}
await cleanup();
assert.ok(stopped);

await new Promise((resolve) => server.close(resolve));
await v1("session.idle", { sessionID: "s1" });
await settle();
rmSync(home, { recursive: true, force: true });

console.log("PASS: the OpenCode plugin reports OpenCode 1 and 2 sessions to Ledge and stays quiet when Ledge is not running");
