/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { getSlashCommandsList, type SlashCommandOption } from "@services/tauri";
import { useSlashCommands } from "./useSlashCommands";

vi.mock("@services/tauri", () => ({
  getSlashCommandsList: vi.fn(),
}));

function command(name: string): SlashCommandOption {
  return {
    name,
    description: null,
    argumentHint: null,
    kind: "skill",
    scope: "user",
    plugin: null,
    invocation: "slash",
  };
}

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

describe("useSlashCommands", () => {
  it("loads commands for the provider and reloads when it changes", async () => {
    vi.mocked(getSlashCommandsList).mockImplementation(async (_workspaceId, provider) =>
      provider === "claude" ? [command("graphify")] : [command("devops-helper")],
    );
    const { result, rerender } = renderHook((props) => useSlashCommands(props), {
      initialProps: { workspaceId: "ws-1" as string | null, provider: "claude" },
    });
    await waitFor(() => expect(result.current.map((item) => item.name)).toEqual(["graphify"]));
    expect(getSlashCommandsList).toHaveBeenCalledWith("ws-1", "claude");

    rerender({ workspaceId: "ws-1", provider: "codex" });
    await waitFor(() => expect(result.current.map((item) => item.name)).toEqual(["devops-helper"]));
  });

  it("refreshes when the window regains focus", async () => {
    vi.mocked(getSlashCommandsList)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([command("new-skill")]);
    const { result } = renderHook(() => useSlashCommands({ workspaceId: null, provider: "claude" }));
    await waitFor(() => expect(getSlashCommandsList).toHaveBeenCalledTimes(1));

    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    await waitFor(() => expect(result.current.map((item) => item.name)).toEqual(["new-skill"]));
  });

  it("ignores stale responses", async () => {
    let resolveFirst: (value: SlashCommandOption[]) => void = () => {};
    vi.mocked(getSlashCommandsList)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce([command("codex-skill")]);
    const { result, rerender } = renderHook((props) => useSlashCommands(props), {
      initialProps: { workspaceId: null as string | null, provider: "claude" },
    });
    rerender({ workspaceId: null, provider: "codex" });
    await waitFor(() => expect(result.current.map((item) => item.name)).toEqual(["codex-skill"]));

    await act(async () => {
      resolveFirst([command("stale")]);
    });
    expect(result.current.map((item) => item.name)).toEqual(["codex-skill"]);
  });

  it("falls back to an empty list on errors", async () => {
    vi.mocked(getSlashCommandsList).mockRejectedValue(new Error("boom"));
    const { result } = renderHook(() => useSlashCommands({ workspaceId: null, provider: "claude" }));
    await waitFor(() => expect(getSlashCommandsList).toHaveBeenCalled());
    expect(result.current).toEqual([]);
  });
});
