// Client for the openade-daemon localhost API.
// Types mirror `openade-core` / `openade-daemon` serde shapes.

export type Harness = "claude-code" | "codex-cli" | "gemini-cli";

export const HARNESSES: { id: Harness; label: string }[] = [
  { id: "claude-code", label: "Claude Code" },
  { id: "codex-cli", label: "Codex CLI" },
  { id: "gemini-cli", label: "Gemini CLI" },
];

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
  base_commit?: string;
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

export interface ArtifactInfo {
  branch: string;
  file: string;
  summary: string;
  markdown: string;
}

const DAEMON_URL =
  (import.meta as { env?: Record<string, string> }).env
    ?.VITE_OPENADE_DAEMON_URL ?? "http://127.0.0.1:7433";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${DAEMON_URL}${path}`, {
    headers: { "content-type": "application/json" },
    ...init,
  });
  if (!res.ok) {
    let detail = "";
    try {
      const body = (await res.json()) as { error?: string };
      detail = body.error ?? "";
    } catch {
      detail = await res.text().catch(() => "");
    }
    throw new Error(detail !== "" ? detail : `daemon returned ${res.status}`);
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

export function getDiff(id: string): Promise<{ diff: string }> {
  return request(`/sessions/${id}/diff`);
}

export function getFiles(id: string): Promise<{ files: string[] }> {
  return request(`/sessions/${id}/files`);
}

export function listProjects(): Promise<{ projects: string[] }> {
  return request("/projects");
}

export function publishArtifact(id: string): Promise<ArtifactInfo> {
  return request(`/sessions/${id}/artifact`, { method: "POST", body: "{}" });
}

export function handoffSession(
  id: string,
  harness: Harness,
): Promise<SessionMeta> {
  return request(`/sessions/${id}/handoff`, {
    method: "POST",
    body: JSON.stringify({ harness }),
  });
}
