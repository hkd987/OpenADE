import { afterEach, describe, expect, it, vi } from "vitest";
import {
  createSession,
  getDiff,
  handoffSession,
  HARNESSES,
  killSession,
  listProjects,
  listSessions,
  publishArtifact,
  sendInput,
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
  it("covers all three harnesses", () => {
    expect(HARNESSES.map((h) => h.id)).toEqual([
      "claude-code",
      "codex-cli",
      "gemini-cli",
    ]);
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
});
