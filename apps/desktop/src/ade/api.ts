export const DAEMON_URL = "http://127.0.0.1:7433";

export type SessionStatus =
  | "starting"
  | "running"
  | "waiting"
  | "completed"
  | "failed"
  | "stopped"
  | "interrupted";

export interface Session {
  id: string;
  title: string;
  prompt: string;
  agent: string;
  mode: "chat" | "tui";
  repo_root: string;
  worktree_path: string;
  branch: string;
  base_branch: string;
  ticket_key?: string;
  ticket_url?: string;
  status: SessionStatus;
  pid?: number;
  exit_code?: number;
  pr_url?: string;
  created_at: string;
  updated_at: string;
  finished_at?: string;
}

export interface ProjectTerminal {
  id: string;
  session_id: string;
  title: string;
  cwd: string;
  status: "running" | "completed" | "failed" | "stopped" | "interrupted";
  pid?: number;
  exit_code?: number;
  created_at: string;
  updated_at: string;
  finished_at?: string;
}

export interface AgentInfo {
  id: string;
  available: boolean;
  path: string;
}

export interface Meta {
  agents: AgentInfo[];
  github_available: boolean;
  data_dir: string;
}

export interface PullRequest {
  number: number;
  title: string;
  url: string;
  state: string;
  isDraft: boolean;
  headRefName: string;
  baseRefName: string;
  reviewDecision: string;
  updatedAt: string;
  author: { login: string };
  labels: { name: string }[];
}

export interface Ticket {
  key: string;
  summary: string;
  status: string;
  assignee: string;
  url: string;
  source: string;
  fetched_at: string;
}

export interface CreateSessionInput {
  title: string;
  prompt: string;
  agent: string;
  mode?: "chat" | "tui";
  repo_root: string;
  base_branch: string;
  ticket_key?: string;
  ticket_url?: string;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${DAEMON_URL}${path}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...init?.headers,
    },
  });
  if (!response.ok) {
    const payload = (await response.json().catch(() => ({}))) as {
      error?: string;
    };
    throw new Error(payload.error ?? `OpenADE daemon returned ${response.status}`);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

export async function health(): Promise<boolean> {
  try {
    await request<{ ok: boolean }>("/api/health");
    return true;
  } catch {
    return false;
  }
}

export const getMeta = () => request<Meta>("/api/meta");

export async function listSessions(): Promise<Session[]> {
  const payload = await request<{ sessions: Session[] }>("/api/sessions");
  return payload.sessions ?? [];
}

export async function listProjects(): Promise<string[]> {
  const payload = await request<{ projects: string[] }>("/api/projects");
  return payload.projects ?? [];
}

export async function scanProjects(root: string): Promise<string[]> {
  const payload = await request<{ projects: string[] }>("/api/projects/scan", {
    method: "POST",
    body: JSON.stringify({ root }),
  });
  return payload.projects ?? [];
}

export const createSession = (input: CreateSessionInput) =>
  request<Session>("/api/sessions", {
    method: "POST",
    body: JSON.stringify(input),
  });

export const sendInput = (id: string, data: string) =>
  request<void>(`/api/sessions/${id}/input`, {
    method: "POST",
    body: JSON.stringify({ data }),
  });

export const sendMessage = (id: string, text: string) =>
  request<Session>(`/api/sessions/${id}/messages`, {
    method: "POST",
    body: JSON.stringify({ text }),
  });

export const resumeTUI = (id: string) =>
  request<Session>(`/api/sessions/${id}/resume-tui`, { method: "POST" });

export const resizeTerminal = (id: string, rows: number, cols: number) =>
  request<void>(`/api/sessions/${id}/resize`, {
    method: "POST",
    body: JSON.stringify({ rows, cols }),
  });

export const stopSession = (id: string) =>
  request<void>(`/api/sessions/${id}/stop`, { method: "POST" });

export async function listTerminals(sessionId: string): Promise<ProjectTerminal[]> {
  const payload = await request<{ terminals: ProjectTerminal[] }>(
    `/api/sessions/${sessionId}/terminals`,
  );
  return payload.terminals ?? [];
}

export const createTerminal = (sessionId: string, options?: { title?: string; kind?: "shell" | "agent"; agent?: string; resume?: boolean }) =>
  request<ProjectTerminal>(`/api/sessions/${sessionId}/terminals`, {
    method: "POST",
    body: JSON.stringify({ title: options?.title ?? "", kind: options?.kind ?? "shell", agent: options?.agent ?? "", resume: options?.resume ?? false }),
  });

export const sendTerminalInput = (id: string, data: string) =>
  request<void>(`/api/terminals/${id}/input`, {
    method: "POST",
    body: JSON.stringify({ data }),
  });

export const resizeProjectTerminal = (id: string, rows: number, cols: number) =>
  request<void>(`/api/terminals/${id}/resize`, {
    method: "POST",
    body: JSON.stringify({ rows, cols }),
  });

export const stopTerminal = (id: string) =>
  request<void>(`/api/terminals/${id}/stop`, { method: "POST" });

export async function getDiff(id: string): Promise<string> {
  return (await request<{ diff: string }>(`/api/sessions/${id}/diff`)).diff;
}

export async function getFiles(id: string): Promise<string[]> {
  return (await request<{ files: string[] }>(`/api/sessions/${id}/files`)).files;
}

export async function listPullRequests(repo: string): Promise<PullRequest[]> {
  const payload = await request<{ pull_requests: PullRequest[] }>(
    `/api/github/pull-requests?repo=${encodeURIComponent(repo)}`,
  );
  return payload.pull_requests ?? [];
}

export async function createPullRequest(input: {
  sessionId: string;
  title: string;
  body: string;
  base: string;
}): Promise<string> {
  const payload = await request<{ url: string }>("/api/github/pull-requests", {
    method: "POST",
    body: JSON.stringify({
      SessionID: input.sessionId,
      Title: input.title,
      Body: input.body,
      Base: input.base,
    }),
  });
  return payload.url;
}

export const getTicket = (key: string) =>
  request<Ticket>(`/api/jira/tickets/${encodeURIComponent(key)}`);

export function streamURL(id: string): string {
  return `${DAEMON_URL.replace(/^http/, "ws")}/api/sessions/${id}/stream`;
}

export function terminalStreamURL(id: string): string {
  return `${DAEMON_URL.replace(/^http/, "ws")}/api/terminals/${id}/stream`;
}

export function projectName(path: string): string {
  return path.split("/").filter(Boolean).at(-1) ?? path;
}

export function relativeTime(value: string): string {
  const elapsed = Date.now() - new Date(value).getTime();
  if (!Number.isFinite(elapsed) || elapsed < 0) return "now";
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}
