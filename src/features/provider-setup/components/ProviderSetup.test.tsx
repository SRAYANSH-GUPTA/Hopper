// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderSetup } from "./ProviderSetup";
import { getProviderSetupStatus, runProviderSetupAction, saveProviderSetup, type ProviderSetupStatus } from "@services/tauri";

vi.mock("@services/tauri", () => ({
  getProviderSetupStatus: vi.fn(), runProviderSetupAction: vi.fn(), saveProviderSetup: vi.fn(),
}));
const status: ProviderSetupStatus = {
  preferences: { completed: false, claudeEnabled: false, antigravityEnabled: false, antigravityAutoApprove: false },
  platform: "linux", supported: true,
  providers: [
    { id: "claude", label: "Claude Code", installed: false, version: null, path: null, authenticated: null },
    { id: "antigravity", label: "Antigravity", installed: true, version: null, path: "/usr/bin/agy", authenticated: null },
  ],
};
beforeEach(() => {
  vi.mocked(getProviderSetupStatus).mockResolvedValue(structuredClone(status));
  vi.mocked(saveProviderSetup).mockResolvedValue({ ok: true });
  vi.mocked(runProviderSetupAction).mockResolvedValue({ message: "Done" });
});
afterEach(() => { cleanup(); vi.resetAllMocks(); });

describe("ProviderSetup", () => {
  it("does not install or sign in automatically and requires a verified selected provider", async () => {
    const configured = vi.fn().mockResolvedValue(undefined);
    render(<ProviderSetup onboarding targetKey="local" onConfigured={configured} />);
    const dialog = await screen.findByRole("dialog", { name: "Provider setup" });
    expect(runProviderSetupAction).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "Antigravity" }));
    const finish = screen.getByRole("button", { name: "Start using Hopper" }) as HTMLButtonElement;
    expect(finish.disabled).toBe(true);
    const bypass = screen.getByRole("checkbox", { name: /Allow Antigravity/ }) as HTMLInputElement;
    expect(bypass.checked).toBe(false);
    vi.mocked(runProviderSetupAction).mockResolvedValue({ message: "Connection verified", verified: true });
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    await waitFor(() => expect(finish.disabled).toBe(false));
    expect(runProviderSetupAction).toHaveBeenCalledWith("antigravity", "verify");
    fireEvent.click(finish);
    await waitFor(() => expect(configured).toHaveBeenCalledWith(expect.objectContaining({ antigravityEnabled: true, antigravityAutoApprove: false })));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("installs only the selected provider after its install button is clicked", async () => {
    render(<ProviderSetup targetKey="local" />);
    fireEvent.click(await screen.findByRole("checkbox", { name: "Claude Code" }));
    fireEvent.click(screen.getByRole("button", { name: "Install Claude Code" }));
    await waitFor(() => expect(runProviderSetupAction).toHaveBeenCalledWith("claude", "install"));
    expect(saveProviderSetup).not.toHaveBeenCalled();
  });

  it("keeps failed connection checks incomplete and retryable", async () => {
    vi.mocked(runProviderSetupAction).mockRejectedValue(new Error("Sign in first"));
    render(<ProviderSetup onboarding targetKey="local" />);
    fireEvent.click(await screen.findByRole("checkbox", { name: "Antigravity" }));
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    expect((await screen.findByRole("alert")).textContent).toContain("Sign in first");
    expect((screen.getByRole("button", { name: "Start using Hopper" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("does not open onboarding again after dismissal", async () => {
    vi.mocked(getProviderSetupStatus).mockResolvedValue({ ...status, preferences: { ...status.preferences, completed: true } });
    render(<ProviderSetup onboarding targetKey="local" />);
    await waitFor(() => expect(getProviderSetupStatus).toHaveBeenCalled());
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("does not launch a local sign-in for a remote host", async () => {
    render(<ProviderSetup remote targetKey="remote:host" />);
    fireEvent.click(await screen.findByRole("checkbox", { name: "Antigravity" }));
    expect((screen.getByRole("button", { name: "Sign in" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/On the host, run/).textContent).toContain("agy");
  });

  it("ignores an old host response after the host changes", async () => {
    let resolveOld!: (value: ProviderSetupStatus) => void;
    vi.mocked(getProviderSetupStatus).mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }));
    const { rerender } = render(<ProviderSetup targetKey="remote:old" />);
    rerender(<ProviderSetup targetKey="remote:new" />);
    await screen.findByRole("checkbox", { name: "Antigravity" });
    resolveOld({ ...status, providers: [] });
    await waitFor(() => expect(screen.getByRole("checkbox", { name: "Antigravity" })).toBeTruthy());
  });
  it("keeps keyboard focus in onboarding and allows Escape without saving", async () => {
    render(<ProviderSetup onboarding targetKey="local" />);
    const dialog = await screen.findByRole("dialog", { name: "Provider setup" });
    const later = screen.getByRole("button", { name: "Set up later" });
    later.focus();
    fireEvent.keyDown(later, { key: "Tab" });
    expect(document.activeElement).toBe(screen.getByRole("checkbox", { name: "Claude Code" }));
    fireEvent.keyDown(within(dialog).getByRole("checkbox", { name: "Claude Code" }), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(saveProviderSetup).not.toHaveBeenCalled();
  });

});
