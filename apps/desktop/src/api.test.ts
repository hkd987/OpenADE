import { afterEach, describe, expect, it, vi } from "vitest";
import {
  createSession,
  getDiff,
  handoffSession,
  harnessLabel,
  HARNESSES,
  killSession,
  listProjects,
  listSessions,
  projectName,
  publishArtifact,
  sendInput,
  splitEntityRef,
  timeAgo,
} from "./api";

function mockFetch(status: number, body: unknown) {
  const fn = vi.fn().mockResolvedValue({
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
    text: () => Promise.resolve(JSON.stringify(body)),
  });
  vi.stubGlobal("fetch", fn);
  return fn;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("api client", () => {
  it("covers all four harnesses with vendor attribution", () => {
    expect(HARNESSES.map((h) => h.id)).toEqual([
      "claude-code",
      "codex-cli",
      "gemini-cli",
      "copilot-cli",
      "opencode",
    ]);
    expect(HARNESSES.map((h) => h.vendor)).toEqual([
      "Anthropic",
      "OpenAI",
      "Google",
      "GitHub",
      "SST",
    ]);
    expect(harnessLabel("opencode")).toBe("OpenCode");
    expect(harnessLabel("gemini-cli")).toBe("Gemini CLI");
    expect(harnessLabel("unknown" as never)).toBe("unknown");
  });

  it("derives project names from repository paths", () => {
    expect(projectName("/repos/payments-api")).toBe("payments-api");
    expect(projectName("/repos/payments-api/")).toBe("payments-api");
    expect(projectName("")).toBe("");
  });

  it("splits entity refs into kind and rest for the memory chip", () => {
    expect(splitEntityRef("repo:acme/payments")).toEqual({
      kind: "repo",
      rest: "acme/payments",
    });
    expect(splitEntityRef("component:default/x")).toEqual({
      kind: "component",
      rest: "default/x",
    });
    expect(splitEntityRef("no-colon")).toEqual({
      kind: "entity",
      rest: "no-colon",
    });
    expect(splitEntityRef(":odd")).toEqual({ kind: "entity", rest: ":odd" });
  });

  it("renders compact relative times", () => {
    const now = new Date("2026-08-11T12:00:00Z");
    expect(timeAgo("2026-08-11T11:59:30Z", now)).toBe("now");
    expect(timeAgo("2026-08-11T11:55:00Z", now)).toBe("5m ago");
    expect(timeAgo("2026-08-11T09:00:00Z", now)).toBe("3h ago");
    expect(timeAgo("2026-08-09T12:00:00Z", now)).toBe("2d ago");
    expect(timeAgo("not a date", now)).toBe("");
    expect(timeAgo("2026-08-12T00:00:00Z", now)).toBe("");
  });

  it("lists sessions from the daemon", async () => {
    const fetch = mockFetch(200, { sessions: [] });
    const result = await listSessions();
    expect(result.sessions).toEqual([]);
    expect(fetch).toHaveBeenCalledWith(
      expect.stringContaining("/sessions"),
      expect.objectContaining({
        headers: { "content-type": "application/json" },
      }),
    );
  });

  it("posts launch requests as JSON", async () => {
    const fetch = mockFetch(201, { id: "abc", state: "running" });
    const meta = await createSession({
      title: "t",
      harness: "claude-code",
      repo_root: "/repo",
    });
    expect(meta.id).toBe("abc");
    const [, init] = fetch.mock.calls[0] as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string).harness).toBe("claude-code");
  });

  it("surfaces daemon error messages", async () => {
    mockFetch(409, { error: "worktree has uncommitted changes" });
    await expect(killSession("abc")).rejects.toThrow(
      "worktree has uncommitted changes",
    );
  });

  it("falls back to the raw body when the error is not JSON", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 502,
        json: () => Promise.reject(new Error("not json")),
        text: () => Promise.resolve("bad gateway"),
      }),
    );
    await expect(killSession("abc")).rejects.toThrow("bad gateway");

    // And to the status code when the body is unreadable entirely.
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 500,
        json: () => Promise.reject(new Error("not json")),
        text: () => Promise.reject(new Error("stream broke")),
      }),
    );
    await expect(killSession("abc")).rejects.toThrow("daemon returned 500");
  });

  it("fetches scrollback and file listings", async () => {
    const fetch = mockFetch(200, { scrollback: "out", files: [] });
    const { getScrollback, getFiles } = await import("./api");
    await getScrollback("abc");
    await getFiles("abc");
    const urls = fetch.mock.calls.map((c) => c[0] as string);
    expect(urls[0]).toContain("/sessions/abc/scrollback");
    expect(urls[1]).toContain("/sessions/abc/files");
  });

  it("handles 204 responses without parsing a body", async () => {
    mockFetch(204, undefined);
    await expect(sendInput("abc", "y\n")).resolves.toBeUndefined();
  });

  it("requests diff, projects, artifact, and handoff endpoints", async () => {
    const fetch = mockFetch(200, {
      diff: "",
      projects: [],
      branch: "b",
      harness: "gemini-cli",
    });
    await getDiff("abc");
    await listProjects();
    await publishArtifact("abc");
    await handoffSession("abc", "gemini-cli");
    const urls = fetch.mock.calls.map((c) => c[0] as string);
    expect(urls[0]).toContain("/sessions/abc/diff");
    expect(urls[1]).toContain("/projects");
    expect(urls[2]).toContain("/sessions/abc/artifact");
    expect(urls[3]).toContain("/sessions/abc/handoff");
    const handoffInit = fetch.mock.calls[3][1] as RequestInit;
    expect(JSON.parse(handoffInit.body as string)).toEqual({
      harness: "gemini-cli",
    });
  });

  it("reads and writes the daemon config (onboarding)", async () => {
    const fetch = mockFetch(200, { onboarded: false, memory_sources: [] });
    const { getConfig, putConfig } = await import("./api");
    await getConfig();
    await putConfig({ memory_repo: "acme/team-memory", onboarded: true });
    const urls = fetch.mock.calls.map((c) => c[0] as string);
    expect(urls[0]).toContain("/config");
    expect(urls[1]).toContain("/config");
    const putInit = fetch.mock.calls[1][1] as RequestInit;
    expect(putInit.method).toBe("PUT");
    expect(JSON.parse(putInit.body as string)).toEqual({
      memory_repo: "acme/team-memory",
      onboarded: true,
    });
  });

  it("shares, lists, reads, and picks up workspace sessions", async () => {
    const fetch = mockFetch(200, {
      id: 7,
      sessions: [],
      session: { id: 7 },
      markdown: "",
      events: [],
    });
    const {
      shareSession,
      pickupSession,
      listWorkspaceSessions,
      getWorkspaceSession,
    } = await import("./api");
    await shareSession("abc");
    await listWorkspaceSessions();
    await listWorkspaceSessions("repo:acme/payments");
    await getWorkspaceSession(7);
    await pickupSession({
      workspace_session_id: 7,
      harness: "gemini-cli",
      repo_root: "/repo",
    });
    const urls = fetch.mock.calls.map((c) => c[0] as string);
    expect(urls[0]).toContain("/sessions/abc/share");
    expect(urls[1]).toMatch(/\/workspace\/sessions$/);
    expect(urls[2]).toContain(
      "/workspace/sessions?entity=repo%3Aacme%2Fpayments",
    );
    expect(urls[3]).toContain("/workspace/sessions/7");
    const pickupInit = fetch.mock.calls[4][1] as RequestInit;
    expect(pickupInit.method).toBe("POST");
    expect(JSON.parse(pickupInit.body as string)).toEqual({
      workspace_session_id: 7,
      harness: "gemini-cli",
      repo_root: "/repo",
    });
  });

  it("fetches file content, skills, and project PRs", async () => {
    const fetch = mockFetch(200, { path: "a.md", content: "x", skills: [], prs: [] });
    const { getFileContent, getSkills, listPrs } = await import("./api");
    await getFileContent("abc", "src/a b.md");
    await getSkills("abc");
    await listPrs("/repos/checkout");
    const urls = fetch.mock.calls.map((c) => c[0] as string);
    expect(urls[0]).toContain("/sessions/abc/file?path=src%2Fa%20b.md");
    expect(urls[1]).toContain("/sessions/abc/skills");
    expect(urls[2]).toContain("/projects/prs?repo=%2Frepos%2Fcheckout");
  });
});
