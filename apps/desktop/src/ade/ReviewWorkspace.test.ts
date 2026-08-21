import { describe, expect, it } from "vitest";
import { parseUnifiedDiff } from "./ReviewWorkspace";

describe("parseUnifiedDiff", () => {
  it("indexes added, modified, and deleted files for the review tree", () => {
    const diff = [
      "diff --git a/src/new.ts b/src/new.ts",
      "new file mode 100644",
      "--- /dev/null",
      "+++ b/src/new.ts",
      "@@ -0,0 +1 @@",
      "+export const value = 1;",
      "diff --git a/README.md b/README.md",
      "index 111..222 100644",
      "--- a/README.md",
      "+++ b/README.md",
      "@@ -1 +1 @@",
      "-Old",
      "+New",
      "diff --git a/old.txt b/old.txt",
      "deleted file mode 100644",
      "--- a/old.txt",
      "+++ /dev/null",
      "@@ -1 +0,0 @@",
      "-gone",
    ].join("\n");
    expect(parseUnifiedDiff(diff).map(({ path, status }) => ({ path, status }))).toEqual([
      { path: "src/new.ts", status: "A" },
      { path: "README.md", status: "M" },
      { path: "old.txt", status: "D" },
    ]);
  });
});
