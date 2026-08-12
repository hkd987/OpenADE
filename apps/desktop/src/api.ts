// Client for the openade-daemon localhost API.
// Types mirror `openade-core` / `openade-daemon` serde shapes.

export type Harness =
  | "claude-code"
  | "codex-cli"
  | "gemini-cli"
  | "copilot-cli"
  | "opencode";

export const HARNESSES: { id: Harness; label: string; vendor: string }[] = [
  { id: "claude-code", label: "Claude Code", vendor: "Anthropic" },
  { id: "codex-cli", label: "Codex CLI", vendor: "OpenAI" },
  { id: "gemini-cli", label: "Gemini CLI", vendor: "Google" },
  { id: "copilot-cli", label: "Copilot CLI", vendor: "GitHub" },
  { id: "opencode", label: "OpenCode", vendor: "SST" },
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

/** Where a session's working directory lives (Xirp's checkout mode). */
export type CheckoutMode = "worktree" | "main";

export interface LaunchSessionRequest {
  title: string;
  harness: Harness;
  repo_root: string;
  checkout?: CheckoutMode;
  entity_ref?: string;
  prompt?: string;
}

/** A reusable agent skill discovered in the session's working directory. */
export interface SkillInfo {
  name: string;
  path: string;
  description: string;
}

/** An open pull request on a project's GitHub remote. */
export interface PrInfo {
  number: number;
  title: string;
  url: string;
  headRefName: string;
  isDraft: boolean;
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

/** A session shared to the team workspace (multiplayer). */
export interface WorkspaceSession {
  id: number;
  title: string;
  harness: string;
  entity_ref?: string | null;
  summary: string;
  shared_by: string;
  uploaded_at: string;
}

/** One transcript event in a shared session's harness-neutral record. */
export interface WorkspaceEvent {
  kind: string;
  payload?: { text?: string };
}

/** Full record of a shared session (read-only Team viewer). */
export interface WorkspaceSessionDetail {
  session: WorkspaceSession;
  markdown: string;
  events: WorkspaceEvent[];
}

/** Pick a shared session up as a new local session (any harness). */
export interface PickupRequest {
  workspace_session_id: number;
  harness: Harness;
  repo_root: string;
  checkout?: CheckoutMode;
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
  server_url: string | null;
  server_token_set: boolean;
  server_workspace: number | null;
  workspace_configured: boolean;
}

/** Settings the onboarding flow saves; env vars still win daemon-side. */
export interface ConfigUpdate {
  backstage_base_url?: string;
  backstage_token?: string;
  memory_repo?: string;
  onboarded: boolean;
  server_url?: string;
  /** Absent = keep the stored member token (like backstage_token). */
  server_token?: string;
  server_workspace?: number;
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

export function getFileContent(
  id: string,
  path: string,
): Promise<{ path: string; content: string }> {
  return request(`/sessions/${id}/file?path=${encodeURIComponent(path)}`);
}

export function getSkills(id: string): Promise<{ skills: SkillInfo[] }> {
  return request(`/sessions/${id}/skills`);
}

export function listPrs(
  repoRoot: string,
): Promise<{ prs: PrInfo[]; note?: string }> {
  return request(`/projects/prs?repo=${encodeURIComponent(repoRoot)}`);
}

export function listProjects(): Promise<{ projects: string[] }> {
  return request("/projects");
}

export function publishArtifact(id: string): Promise<ArtifactInfo> {
  return request(`/sessions/${id}/artifact`, { method: "POST", body: "{}" });
}

export function shareSession(id: string): Promise<WorkspaceSession> {
  return request(`/sessions/${id}/share`, { method: "POST", body: "{}" });
}

export function pickupSession(req: PickupRequest): Promise<SessionMeta> {
  return request("/sessions/pickup", {
    method: "POST",
    body: JSON.stringify(req),
  });
}

export function listWorkspaceSessions(
  entity?: string,
): Promise<{ sessions: WorkspaceSession[] }> {
  const query =
    entity !== undefined ? `?entity=${encodeURIComponent(entity)}` : "";
  return request(`/workspace/sessions${query}`);
}

export function getWorkspaceSession(
  sid: number,
): Promise<WorkspaceSessionDetail> {
  return request(`/workspace/sessions/${sid}`);
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
