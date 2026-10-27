import { connect } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";

const SOCKET = join(homedir(), ".ledge", "hook.sock");

const titles = new Map();
const children = new Set();

const send = (state, sessionID, directory) => {
  if (!sessionID || children.has(sessionID)) return;
  const client = connect(SOCKET);
  client.on("error", () => {});
  client.end(
    `opencode\n${JSON.stringify({ state, sessionID, directory, title: titles.get(sessionID) })}`,
  );
};

const remember = (sessionID, parentID, title) => {
  if (!sessionID) return;
  if (parentID) children.add(sessionID);
  else if (title) titles.set(sessionID, title);
};

const V1_STATES = {
  "permission.asked": "waiting",
  "question.asked": "waiting",
  "permission.replied": "working",
  "question.replied": "working",
  "question.rejected": "working",
  "session.idle": "done",
};

const LedgePlugin = async ({ directory }) => ({
  event: async ({ event }) => {
    const properties = event.properties ?? {};
    const sessionID = properties.sessionID;

    switch (event.type) {
      case "session.created":
      case "session.updated":
        remember(properties.info?.id, properties.info?.parentID, properties.info?.title);
        return;
      case "session.status":
        if (["busy", "retry"].includes(properties.status?.type)) send("working", sessionID, directory);
        return;
      case "session.error":
        send(
          properties.error?.name === "MessageAbortedError" ? "done" : "failed",
          sessionID,
          directory,
        );
        return;
      default:
        if (V1_STATES[event.type]) send(V1_STATES[event.type], sessionID, directory);
    }
  },
});

const V2_STATES = {
  "session.execution.started": "working",
  "permission.asked": "waiting",
  "form.created": "waiting",
  "permission.replied": "working",
  "form.replied": "working",
  "form.cancelled": "working",
  "session.execution.succeeded": "done",
  "session.execution.interrupted": "done",
  "session.execution.failed": "failed",
};

const QUESTION_EVENTS = [
  "session.tool.input.started",
  "session.tool.called",
  "session.tool.success",
  "session.tool.failed",
];

const setup = (context) => {
  const questions = new Set();
  const events = context.event
    .subscribe({
      types: [
        "session.created",
        "session.renamed",
        "session.status",
        ...QUESTION_EVENTS,
        ...Object.keys(V2_STATES),
      ],
    })
    [Symbol.asyncIterator]();

  const listen = async () => {
    for (;;) {
      const { value: event, done } = await events.next();
      if (done) return;
      const data = event.data ?? {};
      const directory = event.location?.directory ?? context.location?.directory;

      if (event.type === "session.created" || event.type === "session.renamed") {
        remember(data.sessionID, data.parentID, data.title);
      } else if (event.type === "session.status") {
        if (["busy", "retry"].includes(data.status?.type)) send("working", data.sessionID, directory);
      } else if (event.type === "session.tool.input.started") {
        if (data.name === "question") questions.add(data.id);
      } else if (event.type === "session.tool.called") {
        if (questions.has(data.id)) send("waiting", data.sessionID, directory);
      } else if (event.type === "session.tool.success" || event.type === "session.tool.failed") {
        if (questions.delete(data.id)) send("working", data.sessionID, directory);
      } else if (V2_STATES[event.type]) {
        send(V2_STATES[event.type], data.sessionID, directory);
      }
    }
  };
  listen().catch(() => {});

  return () => events.return?.();
};

export default { id: "ledge", server: LedgePlugin, setup };
