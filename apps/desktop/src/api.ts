// Client for the openade-daemon localhost API.
// Types mirror `openade-core` / `openade-daemon` serde shapes.

export type Harness = "claude-code" | "codex-cli" | "gemini-cli" | "copilot-cli";

export const HARNESSES: { id: Harness; label: string; vendor: string }[] = [
  { id: "claude-code", label: "Claude Code", vendor: "Anthropic" },
  { id: "codex-cli", label: "Codex CLI", vendor: "OpenAI" },
  { id: "gemini-cli", label: "Gemini CLI", vendor: "Google" },
  { id: "copilot-cli", label: "Copilot CLI", vendor: "GitHub" },
];

export function harnessLabel(id: Harness): string {
  return HARNESSES.find((h) => h.id === id)?.label ?? id;
}

/** Project display name: the repository directory name. */
export function projectName(repoRoot: string): string {
  const parts = repoRoot.split("/").filter((p) => p !== "");
  return parts[parts.length - 1] ?? repoRoot;
}

/**
 * Split an entity ref into its kind and the rest, for the memory chip.
 * `repo:acme/payments` → kind "repo" (GitHub source); anything else (e.g.
 * `component:default/x`) is a catalog kind.
 */
export function splitEntityRef(ref: string): { kind: string; rest: string } {
  const idx = ref.indexOf(":");
  if (idx <= 0) {
    return { kind: "entity", rest: ref };
  }
  return { kind: ref.slice(0, idx), rest: ref.slice(idx + 1) };
}

/** Compact relative time for session cards ("now", "5m ago", "3h ago"). */
export function timeAgo(iso: string, now: Date = new Date()): string {
  const then = new Date(iso).getTime();
  const seconds = Math.floor((now.getTime() - then) / 1000);
  if (Number.isNaN(then) || seconds < 0) {
    return "";
  }
  if (seconds < 60) {
    return "now";
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ago`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  return `${Math.floor(hours / 24)}d ago`;
}

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
  /** Shared memory repo (owner/name) the artifact was also pushed to. */
  shared_repo?: string;
  /** Path of the document inside the shared memory repo. */
  shared_path?: string;
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

/** Daemon configuration + memory status (first-run onboarding). */
export interface DaemonConfig {
  onboarded: boolean;
  backstage_base_url: string | null;
  backstage_token_set: boolean;
  memory_repo: string | null;
  memory_sources: string[];
  gh_found: boolean;
  gh_authenticated: boolean | null;
}

/** Settings the onboarding flow saves; env vars still win daemon-side. */
export interface ConfigUpdate {
  backstage_base_url?: string;
  backstage_token?: string;
  memory_repo?: string;
  onboarded: boolean;
}

export function getConfig(): Promise<DaemonConfig> {
  return request("/config");
}

export function putConfig(update: ConfigUpdate): Promise<DaemonConfig> {
  return request("/config", { method: "PUT", body: JSON.stringify(update) });
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
