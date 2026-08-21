import { act, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { getDiff, getFiles } from "./api";
import { ReviewWorkspace } from "./ReviewWorkspace";

vi.mock("./api", () => ({
  getDiff: vi.fn(),
  getFiles: vi.fn(),
}));

describe("ReviewWorkspace lifecycle", () => {
  it("ignores an older repository response after the selected session changes", async () => {
    let resolveOldDiff!: (value: string) => void;
    let resolveOldFiles!: (value: string[]) => void;
    vi.mocked(getDiff)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOldDiff = resolve; }))
      .mockResolvedValueOnce("diff --git a/new.ts b/new.ts\n@@ -0,0 +1 @@\n+new");
    vi.mocked(getFiles)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOldFiles = resolve; }))
      .mockResolvedValueOnce(["new.ts"]);

    const view = render(<ReviewWorkspace sessionId="old-session" />);
    view.rerender(<ReviewWorkspace sessionId="new-session" />);
    expect((await screen.findAllByText("new.ts")).length).toBeGreaterThan(0);

    await act(async () => {
      resolveOldDiff("diff --git a/stale.ts b/stale.ts\n@@ -0,0 +1 @@\n+stale");
      resolveOldFiles(["stale.ts"]);
      await Promise.resolve();
    });
    expect(screen.queryAllByText("stale.ts")).toHaveLength(0);
    expect(screen.getAllByText("new.ts").length).toBeGreaterThan(0);
  });
});
