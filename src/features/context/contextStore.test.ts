import { describe, expect, it } from "vitest";
import {
  buildImportedHandoffPrompt,
  isDesignImplementationHandoff,
  type ImportedHandoffContext,
} from "./contextStore";

const designContext: ImportedHandoffContext = {
  source: "Claude Web",
  title: "Dashboard redesign",
  sourceUrl: "https://claude.ai/chat/example",
  importedPath: "/workspace/.hopper/imports/design-123",
  messages: [
    { role: "user", content: "Create a responsive dashboard design." },
    { role: "assistant", content: "I created the React implementation and preview." },
  ],
  artifacts: [
    { path: "dashboard/src/App.tsx", sizeBytes: 1024, sha256: "abc123" },
    { path: "dashboard-preview.png", sizeBytes: 2048, sha256: "def456" },
  ],
};

describe("imported context handoff", () => {
  it("recognizes a Claude Code design implementation handoff", () => {
    expect(isDesignImplementationHandoff(designContext, "Claude Code")).toBe(true);
    expect(isDesignImplementationHandoff(designContext, "Codex")).toBe(false);
  });

  it("builds a structured Claude Code implementation request with concrete artifact paths", () => {
    const prompt = buildImportedHandoffPrompt(designContext, "Claude Code");

    expect(prompt).toContain("### Implementation Request");
    expect(prompt).toContain("Implement the imported design in this repository.");
    expect(prompt).toContain(
      "/workspace/.hopper/imports/design-123/artifacts/dashboard/src/App.tsx",
    );
    expect(prompt).toContain("### Acceptance Criteria");
    expect(prompt).toContain("existing component and token system");
    expect(prompt).toContain("not executed automatically");
  });

  it("keeps non-design imports on the generic handoff path", () => {
    const prompt = buildImportedHandoffPrompt(
      {
        source: "ChatGPT",
        title: "API migration notes",
        sourceUrl: null,
        importedPath: "/workspace/.hopper/imports/notes-123/",
        messages: [{ role: "user", content: "Update the API migration plan." }],
        artifacts: [{ path: "notes.md", sizeBytes: 128, sha256: "987xyz" }],
      },
      "Claude Code",
    );

    expect(prompt).not.toContain("### Implementation Request");
    expect(prompt).toContain("Implement or continue the imported work in this repository.");
    expect(prompt).toContain(
      "/workspace/.hopper/imports/notes-123/artifacts/notes.md",
    );
  });
});
