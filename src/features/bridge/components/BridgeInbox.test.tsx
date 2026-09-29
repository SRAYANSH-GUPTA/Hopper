// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BridgeInbox } from "./BridgeInbox";
import { bridgeGetImport, bridgeListImports, bridgeMaterializeImport, bridgeReadArtifact, type BridgeImport } from "@services/tauri";
import { savePendingHandoff } from "@/features/context/contextStore";

vi.mock("@services/tauri", () => ({
  bridgeGetImport: vi.fn(), bridgeListImports: vi.fn(), bridgeMaterializeImport: vi.fn(),
  bridgeReadArtifact: vi.fn(), bridgeImportFile: vi.fn(), pickBridgeImportFile: vi.fn(),
}));
vi.mock("@services/events", () => ({ subscribeSidebarBrowserDownload: () => () => {} }));
vi.mock("@/features/context/contextStore", async (original) => ({
  ...await original<typeof import("@/features/context/contextStore")>(),
  savePendingHandoff: vi.fn(),
}));

const imported: BridgeImport = {
  id: "design", source: "file-import", title: "Portfolio.html", createdAt: "2026-09-30T12:00:00Z",
  messageCount: 1, conversation: { messages: [{ role: "user", content: "Build my portfolio." }] },
  artifacts: [{ path: "Portfolio.html", sizeBytes: 250, sha256: "abc123", mimeType: "text/html" }],
};
const props = {
  workspaces: [{ id: "workspace", name: "Hopper", path: "/workspace", connected: true, settings: { sidebarCollapsed: false } }],
  activeWorkspaceId: "workspace", onSelectWorkspace: vi.fn(), onProviderSwitch: vi.fn(), onAddAgent: vi.fn(),
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(bridgeListImports).mockResolvedValue([imported]);
  vi.mocked(bridgeGetImport).mockResolvedValue(imported);
  vi.mocked(bridgeReadArtifact).mockResolvedValue({
    path: "Portfolio.html", mimeType: "text/html", sizeBytes: 250,
    contentBase64: btoa('<h1>Portfolio design</h1><script>window.parent.bad = true</script><a href="https://example.com">Link</a><meta http-equiv="refresh" content="0; url=https://example.com">'),
  });
  vi.mocked(bridgeMaterializeImport).mockResolvedValue({ path: "/workspace/.hopper/imports/design", artifactCount: 1 });
});
afterEach(cleanup);

describe("Bridge review and handoff", () => {
  it("opens a static HTML preview without copying files or enabling scripts", async () => {
    render(<BridgeInbox {...props} />);
    const frame = await screen.findByTitle("Preview of Portfolio.html");
    expect(frame.getAttribute("sandbox")).toBe("");
    const document = new DOMParser().parseFromString(frame.getAttribute("srcdoc")!, "text/html");
    expect(document.querySelector("h1")?.textContent).toBe("Portfolio design");
    expect(document.querySelector("script")).toBeNull();
    expect(document.querySelector("a")?.hasAttribute("href")).toBe(false);
    expect(document.querySelector('meta[http-equiv="refresh"]')).toBeNull();
    expect(document.head.firstElementChild?.getAttribute("content")).toContain("default-src 'none'");
    expect(bridgeMaterializeImport).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Source" }));
    expect(screen.queryByTitle("Preview of Portfolio.html")).toBeNull();
    expect(screen.getByText(/<h1>Portfolio design/)).toBeTruthy();
  });

  it("reuses a successful copy when continuing in Claude Code", async () => {
    render(<BridgeInbox {...props} />);
    await screen.findByTitle("Preview of Portfolio.html");
    fireEvent.click(screen.getByRole("button", { name: "Copy files only" }));
    await screen.findByRole("status");
    fireEvent.change(screen.getByLabelText("Agent"), { target: { value: "claude" } });
    fireEvent.click(screen.getByRole("button", { name: "Continue in Claude Code" }));
    await waitFor(() => expect(props.onAddAgent).toHaveBeenCalledWith(props.workspaces[0]));
    expect(bridgeMaterializeImport).toHaveBeenCalledTimes(1);
    expect(props.onProviderSwitch).toHaveBeenCalledWith("claude");
    expect(savePendingHandoff).toHaveBeenCalledWith("workspace", expect.stringContaining("Implement the imported design"));
  });

  it("keeps the action available after a failed copy and does not launch an agent", async () => {
    vi.mocked(bridgeMaterializeImport).mockRejectedValueOnce(new Error("Workspace is read-only."));
    render(<BridgeInbox {...props} />);
    await screen.findByTitle("Preview of Portfolio.html");
    fireEvent.click(screen.getByRole("button", { name: "Continue in Codex" }));
    expect((await screen.findByRole("alert")).textContent).toContain("Workspace is read-only.");
    expect(props.onAddAgent).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Continue in Codex" }));
    await waitFor(() => expect(props.onAddAgent).toHaveBeenCalledTimes(1));
  });

  it("ignores a late response for an import the user has left", async () => {
    const other = { ...imported, id: "other", title: "Other design" };
    vi.mocked(bridgeListImports).mockResolvedValue([imported, other]);
    let resolveFirst!: (value: BridgeImport) => void;
    vi.mocked(bridgeGetImport).mockImplementation((id) => id === "design"
      ? new Promise((resolve) => { resolveFirst = resolve; })
      : Promise.resolve(other));
    render(<BridgeInbox {...props} />);
    fireEvent.click(await screen.findByRole("button", { name: /Other design/ }));
    await screen.findByRole("heading", { name: "Other design" });
    await act(async () => resolveFirst(imported));
    expect(screen.queryByRole("heading", { name: "Portfolio.html" })).toBeNull();
    expect(screen.getByRole("heading", { name: "Other design" })).toBeTruthy();
  });
});
