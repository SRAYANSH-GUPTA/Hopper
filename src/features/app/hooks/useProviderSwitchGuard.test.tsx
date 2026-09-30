// @vitest-environment jsdom
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getProviderSetupStatus, type ProviderSetupStatus } from "@services/tauri";
import { useProviderSwitchGuard } from "./useProviderSwitchGuard";

vi.mock("@services/tauri", () => ({ getProviderSetupStatus: vi.fn() }));

const status: ProviderSetupStatus = {
  preferences: {
    completed: true,
    codexEnabled: true,
    claudeEnabled: false,
    antigravityEnabled: false,
    antigravityAutoApprove: false,
  },
  platform: "linux",
  supported: true,
  providers: [
    { id: "codex", label: "Codex", installed: true, version: "1", path: "/bin/codex", authenticated: true },
    { id: "claude", label: "Claude Code", installed: false, version: null, path: null, authenticated: null },
    { id: "antigravity", label: "Antigravity", installed: true, version: "1", path: "/bin/agy", authenticated: null },
  ],
};

beforeEach(() => {
  vi.mocked(getProviderSetupStatus).mockResolvedValue(structuredClone(status));
});

describe("useProviderSwitchGuard", () => {
  it("switches immediately when the provider is ready", async () => {
    const onSwitch = vi.fn();
    const { result } = renderHook(() => useProviderSwitchGuard(onSwitch));

    await act(async () => {
      await result.current.requestSwitch("codex");
    });

    await waitFor(() => expect(onSwitch).toHaveBeenCalledWith("codex"));
    expect(result.current.requestedProviderId).toBeNull();
  });

  it("opens setup instead of switching when a provider is missing", async () => {
    const onSwitch = vi.fn();
    const { result } = renderHook(() => useProviderSwitchGuard(onSwitch));

    await act(async () => {
      await result.current.requestSwitch("claude");
    });

    await waitFor(() => expect(result.current.requestedProviderId).toBe("claude"));
    expect(onSwitch).not.toHaveBeenCalled();
  });

  it("opens setup when an installed provider reports a signed-out account", async () => {
    vi.mocked(getProviderSetupStatus).mockResolvedValue({
      ...status,
      providers: status.providers.map((provider) => provider.id === "codex"
        ? { ...provider, authenticated: false }
        : provider),
    });
    const onSwitch = vi.fn();
    const { result } = renderHook(() => useProviderSwitchGuard(onSwitch));

    await act(async () => {
      await result.current.requestSwitch("codex");
    });

    await waitFor(() => expect(result.current.requestedProviderId).toBe("codex"));
    expect(onSwitch).not.toHaveBeenCalled();
  });
});
