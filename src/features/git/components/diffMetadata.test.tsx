// @vitest-environment jsdom
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { FileDiffMetadata } from "@pierre/diffs";
import type { ReactNode } from "react";
import { DiffCard } from "./GitDiffViewerDiffCard";
import { PierreDiffBlock } from "./PierreDiffBlock";

const { renderDiff } = vi.hoisted(() => ({ renderDiff: vi.fn() }));
vi.mock("@pierre/diffs/react", () => ({
  FileDiff: ({ fileDiff }: { fileDiff: FileDiffMetadata }) => {
    renderDiff(fileDiff);
    return null;
  },
  WorkerPoolContextProvider: ({ children }: { children: ReactNode }) => children,
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const patch = "diff --git a/manifest.json b/manifest.json\nnew file mode 100644\n--- /dev/null\n+++ b/manifest.json\n@@ -0,0 +1,1 @@\n+{}\n";

describe("diff renderer metadata", () => {
  it("keeps parsed line arrays when the backend returns null file contents", () => {
    const entry = JSON.parse(JSON.stringify({
      path: "manifest.json", status: "A", diff: patch, oldLines: null, newLines: null,
    }));
    render(<DiffCard entry={entry} isSelected={false} diffStyle="split"
      isLoading={false} ignoreWhitespaceChanges={false} showRevert={false}
      interactiveSelectionEnabled={false} />);
    const metadata = renderDiff.mock.calls[0][0] as FileDiffMetadata;
    expect(metadata.deletionLines).toEqual([]);
    expect(metadata.additionLines.join("")).toContain("{}");
  });

  it("keeps patch offsets aligned instead of substituting complete file contents", () => {
    const diff = "diff --git a/file.ts b/file.ts\n--- a/file.ts\n+++ b/file.ts\n@@ -20,1 +20,1 @@\n-before\n+after\n";
    render(<PierreDiffBlock diff={diff} displayPath="file.ts"
      oldLines={["unrelated first line", "before"]}
      newLines={["unrelated first line", "after"]} />);
    const metadata = renderDiff.mock.calls[0][0] as FileDiffMetadata;
    expect(metadata.isPartial).toBe(true);
    expect(metadata.deletionLines.join("").trim()).toBe("before");
    expect(metadata.additionLines.join("").trim()).toBe("after");
  });
});
