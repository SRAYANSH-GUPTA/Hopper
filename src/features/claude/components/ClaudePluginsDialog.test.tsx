// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { claudePluginAction, claudePluginsList, type ClaudePlugin } from "@services/tauri";
import { ClaudePluginsDialog } from "./ClaudePluginsDialog";

vi.mock("@services/tauri", () => ({ claudePluginAction: vi.fn(), claudePluginsList: vi.fn() }));

const plugin: ClaudePlugin = {
  id: "review@official", name: "Review", marketplace: "official", version: "1.2.3",
  scope: "user", enabled: true, projectPath: null, canModify: true,
};

beforeEach(() => {
  vi.mocked(claudePluginsList).mockResolvedValue({ plugins: [plugin] });
  vi.mocked(claudePluginAction).mockResolvedValue({ message: "Plugin updated." });
});
afterEach(() => { cleanup(); vi.resetAllMocks(); });

describe("ClaudePluginsDialog", () => {
  it("shows actual enabled, disabled, and unknown states and keeps managed plugins read-only", async () => {
    vi.mocked(claudePluginsList).mockResolvedValue({ plugins: [
      plugin,
      { ...plugin, id: "lint@official", name: "Lint", scope: "project", enabled: false, projectPath: "/project" },
      { ...plugin, id: "managed", name: "Managed", scope: "managed", enabled: null, canModify: false },
    ] });
    render(<ClaudePluginsDialog workspaceId="ws-1" onClose={vi.fn()} />);

    const review = await screen.findByRole("listitem", { name: "Review, user scope" });
    expect(within(review).getByText("Enabled")).toBeTruthy();
    expect(within(review).getByText("v1.2.3")).toBeTruthy();
    expect(within(screen.getByRole("listitem", { name: "Lint, project scope" })).getByText("Disabled")).toBeTruthy();
    const managed = screen.getByRole("listitem", { name: "Managed, managed scope" });
    expect(within(managed).getByText("Status unavailable")).toBeTruthy();
    expect(within(managed).queryByRole("button")).toBeNull();
    expect(claudePluginAction).not.toHaveBeenCalled();
  });

  it("scopes actions to the plugin and workspace, blocks duplicate clicks, and refreshes discovery", async () => {
    let finish!: (result: { message: string }) => void;
    vi.mocked(claudePluginAction).mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const onChanged = vi.fn();
    render(<ClaudePluginsDialog workspaceId="ws-1" onClose={vi.fn()} onChanged={onChanged} />);
    const disable = await screen.findByRole("button", { name: "Disable Review" });
    fireEvent.click(disable);
    fireEvent.click(disable);
    expect(claudePluginAction).toHaveBeenCalledOnce();
    expect(claudePluginAction).toHaveBeenCalledWith({ workspaceId: "ws-1", action: "disable", plugin: "review@official", scope: "user" });
    vi.mocked(claudePluginsList).mockResolvedValue({ plugins: [{ ...plugin, enabled: false }] });

    await act(async () => { finish({ message: "Plugin disabled." }); });

    expect(onChanged).toHaveBeenCalledWith("ws-1");
    expect(claudePluginsList).toHaveBeenCalledTimes(2);
    expect(await screen.findByRole("button", { name: "Enable Review" })).toBeTruthy();
    expect(screen.getByText(/Plugin disabled\. Changes apply to the next turn/)).toBeTruthy();
  });

  it("requires a marketplace ID and sends the explicitly selected installation scope", async () => {
    render(<ClaudePluginsDialog workspaceId="ws-1" onClose={vi.fn()} />);
    await screen.findByText("review@official");
    const input = screen.getByLabelText("Plugin ID");
    fireEvent.change(input, { target: { value: "https://unconfigured.example/plugin" } });
    fireEvent.click(screen.getByRole("button", { name: "Install plugin" }));
    expect(screen.getByRole("alert").textContent).toContain("name@marketplace");
    expect(claudePluginAction).not.toHaveBeenCalled();
    fireEvent.change(input, { target: { value: "tools@official" } });
    fireEvent.change(screen.getByLabelText("Install scope"), { target: { value: "local" } });
    fireEvent.click(screen.getByRole("button", { name: "Install plugin" }));

    await waitFor(() => expect(claudePluginAction).toHaveBeenCalledWith({ workspaceId: "ws-1", action: "install", plugin: "tools@official", scope: "local" }));
    await waitFor(() => expect((input as HTMLInputElement).value).toBe(""));
  });

  it("preserves newer install text while an earlier request finishes", async () => {
    let finish!: (result: { message: string }) => void;
    vi.mocked(claudePluginAction).mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    render(<ClaudePluginsDialog workspaceId="ws-1" onClose={vi.fn()} />);
    await screen.findByText("review@official");
    const input = screen.getByLabelText("Plugin ID") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "first@official" } });
    fireEvent.click(screen.getByRole("button", { name: "Install plugin" }));
    fireEvent.change(input, { target: { value: "next@official" } });
    await act(async () => { finish({ message: "Installed." }); });
    expect(input.value).toBe("next@official");
  });

  it("confirms uninstall inside the dialog and allows cancelling with Escape", async () => {
    const onClose = vi.fn();
    render(<ClaudePluginsDialog workspaceId="ws-1" onClose={onClose} />);
    const uninstall = await screen.findByRole("button", { name: "Uninstall Review" });
    fireEvent.click(uninstall);
    expect(claudePluginAction).not.toHaveBeenCalled();
    expect(screen.getByText(/Its saved data will be kept/)).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel uninstall" }));
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(screen.queryByRole("button", { name: "Confirm uninstall" })).toBeNull();
    expect(document.activeElement).toBe(uninstall);
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.click(uninstall);
    fireEvent.click(screen.getByRole("button", { name: "Confirm uninstall" }));
    await waitFor(() => expect(claudePluginAction).toHaveBeenCalledWith({ workspaceId: "ws-1", action: "uninstall", plugin: "review@official", scope: "user" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Confirm uninstall" })).toBeNull());
  });

  it("shows list failures and allows retrying into the empty state", async () => {
    vi.mocked(claudePluginsList).mockRejectedValueOnce(new Error("Claude is unavailable"));
    render(<ClaudePluginsDialog workspaceId={null} onClose={vi.fn()} />);
    expect((await screen.findByRole("alert")).textContent).toContain("Claude is unavailable");
    vi.mocked(claudePluginsList).mockResolvedValue({ plugins: [] });
    fireEvent.click(screen.getByRole("button", { name: "Retry loading plugins" }));
    expect(await screen.findByText(/No plugins are installed/)).toBeTruthy();
    expect((screen.getByRole("option", { name: /Project —/ }) as HTMLOptionElement).disabled).toBe(true);
    expect((screen.getByRole("option", { name: /Local —/ }) as HTMLOptionElement).disabled).toBe(true);
  });

  it("retains install input after an action error so the user can retry", async () => {
    vi.mocked(claudePluginAction).mockRejectedValueOnce(new Error("Marketplace unavailable"));
    render(<ClaudePluginsDialog workspaceId="ws-1" onClose={vi.fn()} />);
    await screen.findByText("review@official");
    const input = screen.getByLabelText("Plugin ID") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "tools@official" } });
    fireEvent.click(screen.getByRole("button", { name: "Install plugin" }));
    expect((await screen.findByRole("alert")).textContent).toContain("Marketplace unavailable");
    expect(input.value).toBe("tools@official");
    fireEvent.click(screen.getByRole("button", { name: "Install plugin" }));
    await waitFor(() => expect(claudePluginAction).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(input.value).toBe(""));
  });

  it("ignores an old workspace list after switching to another workspace", async () => {
    let finish!: (result: { plugins: ClaudePlugin[] }) => void;
    vi.mocked(claudePluginsList).mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
    const { rerender } = render(<ClaudePluginsDialog workspaceId="old" onClose={vi.fn()} />);
    vi.mocked(claudePluginsList).mockResolvedValue({ plugins: [] });
    rerender(<ClaudePluginsDialog workspaceId="new" onClose={vi.fn()} />);
    await screen.findByText(/No plugins are installed/);
    await act(async () => { finish({ plugins: [plugin] }); });
    expect(screen.queryByText("review@official")).toBeNull();
    expect(screen.getByText(/No plugins are installed/)).toBeTruthy();
  });

  it("refreshes discovery for a completed old action without changing the new workspace dialog", async () => {
    let finish!: (result: { message: string }) => void;
    vi.mocked(claudePluginAction).mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
    const onChanged = vi.fn();
    const { rerender } = render(<ClaudePluginsDialog workspaceId="old" onClose={vi.fn()} onChanged={onChanged} />);
    await screen.findByText("review@official");
    fireEvent.change(screen.getByLabelText("Plugin ID"), { target: { value: "old@official" } });
    fireEvent.click(screen.getByRole("button", { name: "Install plugin" }));
    vi.mocked(claudePluginsList).mockResolvedValue({ plugins: [] });
    rerender(<ClaudePluginsDialog workspaceId="new" onClose={vi.fn()} onChanged={onChanged} />);
    await screen.findByText(/No plugins are installed/);
    fireEvent.change(screen.getByLabelText("Plugin ID"), { target: { value: "new@official" } });
    await act(async () => { finish({ message: "Old workspace updated." }); });
    expect(onChanged).toHaveBeenCalledWith("old");
    expect((screen.getByLabelText("Plugin ID") as HTMLInputElement).value).toBe("new@official");
    expect(screen.queryByText(/Old workspace updated/)).toBeNull();
    expect(claudePluginsList).toHaveBeenCalledTimes(2);
  });

  it("portals to the body, traps keyboard focus including the scope select, and restores focus", async () => {
    const trigger = document.createElement("button");
    document.body.append(trigger);
    trigger.focus();
    const onClose = vi.fn();
    const { container, unmount } = render(<ClaudePluginsDialog workspaceId="ws-1" onClose={onClose} />);
    await screen.findByText("review@official");
    const dialog = screen.getByRole("dialog", { name: "Claude Code plugins" });
    expect(container.contains(dialog)).toBe(false);
    const close = screen.getByRole("button", { name: "Close plugins" });
    expect(document.activeElement).toBe(close);
    fireEvent.keyDown(close, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(screen.getByLabelText("Install scope"));
    fireEvent.keyDown(document.activeElement!, { key: "Tab" });
    expect(document.activeElement).toBe(close);
    fireEvent.keyDown(close, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
    unmount();
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });
});
