export type State = "working" | "waiting" | "completed" | "failed" | "idle";

export interface SessionEvent {
  session_id: string;
  agent: string;
  project_name: string;
  title: string | null;
  cwd: string;
  state: State;
  started_at: number;
  updated_at: number;
}

export const STATE_TEXT: Record<State, string> = {
  working: "Running",
  waiting: "Needs you",
  completed: "Finished",
  failed: "Failed",
  idle: "Idle",
};

export const SEVERITY: State[] = ["waiting", "failed", "working", "completed", "idle"];

export const COUNTED: State[] = ["working", "waiting", "completed", "failed"];

export function worstOf(group: SessionEvent[]): State {
  return SEVERITY.find((state) => group.some((s) => s.state === state)) ?? "idle";
}

export function byAgent(sessions: SessionEvent[]): Map<string, SessionEvent[]> {
  const groups = new Map<string, SessionEvent[]>();
  for (const session of sessions) {
    groups.set(session.agent, [...(groups.get(session.agent) ?? []), session]);
  }
  return groups;
}

export function isLive(state: State) {
  return state === "working" || state === "waiting";
}

export function elapsed(session: SessionEvent): string {
  const end = isLive(session.state) ? Date.now() : session.updated_at;
  const total = Math.max(0, Math.floor((end - session.started_at) / 1000));
  if (total < 60) return `${total}s`;

  const minutes = Math.floor(total / 60);
  const seconds = `${total % 60}`.padStart(2, "0");
  if (minutes < 60) return `${minutes}m ${seconds}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}
