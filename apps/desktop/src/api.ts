// Client for the openade-daemon localhost API.
// Types mirror `openade-core` / `openade-daemon` serde shapes.

export type Harness = "claude-code" | "codex-cli" | "gemini-cli";

export type SessionState =
  | "idle"
  | "running"
  | "needs-input"
  | "completed"
  | "failed";

export interface SessionMeta {
  id: string;
  title: string;
  harness: Harness;
  repo_root: string;
  worktree_path?: string;
  branch?: string;
  entity_ref?: string;
  state: SessionState;
  created_at: string;
  updated_at: string;
}

export interface LaunchSessionRequest {
  title: string;
  harness: Harness;
  repo_root: string;
  entity_ref?: string;
  prompt?: string;
}

const DAEMON_URL =
  (import.meta as { env?: Record<string, string> }).env?.VITE_OPENADE_DAEMON_URL ??
  "http://127.0.0.1:7433";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${DAEMON_URL}${path}`, {
    headers: { "content-type": "application/json" },
    ...init,
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(`daemon ${res.status}: ${body}`);
  }
  if (res.status === 204) {
    return undefined as T;
  }
  return (await res.json()) as T;
}

export function listSessions(): Promise<{ sessions: SessionMeta[] }> {
  return request("/sessions");
}

export function createSession(req: LaunchSessionRequest): Promise<SessionMeta> {
  return request("/sessions", { method: "POST", body: JSON.stringify(req) });
}

export function getScrollback(id: string): Promise<{ scrollback: string }> {
  return request(`/sessions/${id}/scrollback`);
}

export function sendInput(id: string, data: string): Promise<void> {
  return request(`/sessions/${id}/input`, {
    method: "POST",
    body: JSON.stringify({ data }),
  });
}

export function killSession(id: string): Promise<SessionMeta> {
  return request(`/sessions/${id}`, { method: "DELETE" });
}
