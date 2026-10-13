/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { providerUsageOutput } from "@services/tauri";
import { ClaudeCommandDialog, type ClaudeCommandDialogProps } from "./ClaudeCommandDialog";

vi.mock("@services/tauri", () => ({ providerUsageOutput: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

const props = (overrides: Partial<ClaudeCommandDialogProps> = {}): ClaudeCommandDialogProps => ({
  kind: "help", contextUsage: null, models: [{ id: "sonnet", displayName: "Sonnet", model: "claude-sonnet" }],
  selectedModelId: "sonnet", accessMode: "current", isPlanMode: false, commands: [],
  onClose: vi.fn(), onInsert: vi.fn(), onSelectModel: vi.fn(), onSelectAccessMode: vi.fn(), ...overrides,
});

describe("Claude command dialogs", () => {
  it("shows local token counts separately from the CLI usage report", async () => {
    vi.mocked(providerUsageOutput).mockResolvedValue("Plan limits unavailable for this account.");
    const counts = { inputTokens: 100, outputTokens: 20, totalTokens: 120, cachedInputTokens: 10, reasoningOutputTokens: 0 };
    render(<ClaudeCommandDialog {...props({ kind: "usage", workspacePath: "/project", contextUsage: { total: counts, last: counts, modelContextWindow: 200000 } })} />);
    expect(screen.getByText("Total tokens")).toBeTruthy();
    expect(screen.getAllByText("120")).toHaveLength(2);
    await screen.findByText("Plan limits unavailable for this account.");
    expect(providerUsageOutput).toHaveBeenCalledWith("claude", "/project");
    expect(screen.getByText(/separate from your account/)).toBeTruthy();
  });
  it("shows errors with retry and ignores an old workspace response", async () => {
    let old!: (text: string) => void;
    vi.mocked(providerUsageOutput).mockImplementationOnce(() => new Promise((resolve) => { old = resolve; }))
      .mockRejectedValueOnce(new Error("CLI unavailable")).mockResolvedValueOnce("Updated usage");
    const { rerender } = render(<ClaudeCommandDialog {...props({ kind: "usage", workspacePath: "/old" })} />);
    rerender(<ClaudeCommandDialog {...props({ kind: "usage", workspacePath: "/new" })} />);
    await screen.findByRole("alert");
    await act(async () => { old("Wrong workspace usage"); });
    expect(screen.queryByText("Wrong workspace usage")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await screen.findByText("Updated usage");
  });
  it("selects a model and closes without a provider call", () => {
    const options = props({ kind: "model" });
    render(<ClaudeCommandDialog {...options} />);
    fireEvent.click(screen.getByRole("button", { name: /Sonnet/ }));
    expect(options.onSelectModel).toHaveBeenCalledWith("sonnet");
    expect(options.onClose).toHaveBeenCalledOnce();
    expect(providerUsageOutput).not.toHaveBeenCalled();
  });
  it("requires an explicit choice before changing permissions", () => {
    const options = props({ kind: "permissions" });
    render(<ClaudeCommandDialog {...options} />);
    expect(options.onSelectAccessMode).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /Full access/ }));
    expect(options.onSelectAccessMode).toHaveBeenCalledWith("full-access");
  });
  it("filters namespaced plugin commands and inserts the selected command", () => {
    const options = props({ kind: "skills", commands: [{ name: "reviewer:check", description: "Review code", argumentHint: "[path]", kind: "skill", scope: "plugin", plugin: "reviewer", invocation: "slash" }] });
    render(<ClaudeCommandDialog {...options} />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "none" } });
    expect(screen.getByText("No commands match your search.")).toBeTruthy();
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "review" } });
    fireEvent.click(screen.getByRole("button", { name: /reviewer:check/ }));
    expect(options.onInsert).toHaveBeenCalledWith("/reviewer:check ");
  });
  it("focuses the dialog, traps Tab, and supports Escape", async () => {
    const options = props({ kind: "help" });
    render(<ClaudeCommandDialog {...options} />);
    const close = screen.getByRole("button", { name: "Close Claude commands" });
    await waitFor(() => expect(document.activeElement).toBe(close));
    fireEvent.keyDown(close, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(screen.getAllByRole("button").slice(-1)[0]);
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(options.onClose).toHaveBeenCalledOnce();
  });
});
