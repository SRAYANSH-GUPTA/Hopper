// @vitest-environment jsdom
import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ModelOption, SendMessageResult, WorkspaceInfo } from "../../../types";
import { generateRunMetadata } from "../../../services/tauri";
import { useWorkspaceHome } from "./useWorkspaceHome";

vi.mock("../../../services/tauri", () => ({
  generateRunMetadata: vi.fn(),
}));

const workspace: WorkspaceInfo = {
  id: "ws-1",
  name: "Project",
  path: "/tmp/project",
  connected: true,
  kind: "main",
  settings: { sidebarCollapsed: false },
};

const worktreeWorkspace: WorkspaceInfo = {
  id: "wt-1",
  name: "feat/test",
  path: "/tmp/project/worktrees/feat-test",
  connected: true,
  kind: "worktree",
  parentId: "ws-1",
  worktree: { branch: "feat/test" },
  settings: { sidebarCollapsed: false },
};

const models: ModelOption[] = [
  {
    id: "id-1",
    model: "gpt-5.1-max",
    displayName: "GPT-5.1 Max",
    description: "Test model",
    supportedReasoningEfforts: [
      { reasoningEffort: "low", description: "Low effort" },
      { reasoningEffort: "medium", description: "Medium effort" },
      { reasoningEffort: "high", description: "High effort" },
    ],
    defaultReasoningEffort: "medium",
    isDefault: false,
  },
];

function createOptions() {
  vi.mocked(generateRunMetadata).mockResolvedValue({
    title: "Test run",
    worktreeName: "feat/test",
  });
  return {
    activeWorkspace: workspace,
    models,
    selectedModelId: "id-1",
    addWorktreeAgent: vi.fn().mockResolvedValue(worktreeWorkspace),
    connectWorkspace: vi.fn().mockResolvedValue(undefined),
    startThreadForWorkspace: vi.fn().mockResolvedValue("thread-1"),
    sendUserMessageToThread: vi.fn<Parameters<typeof useWorkspaceHome>[0]["sendUserMessageToThread"]>()
      .mockResolvedValue({ status: "sent" }),
  };
}

describe("useWorkspaceHome", () => {
  it.each([
    { mode: "local", outcome: "blocked" },
    { mode: "local", outcome: "steer_failed" },
    { mode: "local", outcome: "rejected" },
    { mode: "worktree", outcome: "blocked" },
    { mode: "worktree", outcome: "steer_failed" },
    { mode: "worktree", outcome: "rejected" },
  ] as const)("retains the draft and reports an unsuccessful $mode run when $outcome", async ({ mode, outcome }) => {
    const options = createOptions();
    if (outcome === "rejected") options.sendUserMessageToThread.mockRejectedValue(new Error("Connection failed"));
    else options.sendUserMessageToThread.mockResolvedValue({ status: outcome });
    const { result } = renderHook(() => useWorkspaceHome(options));
    act(() => {
      result.current.setRunMode(mode);
      result.current.toggleModelSelection("id-1");
      result.current.setDraft("  Keep this draft  ");
    });

    await act(async () => {
      expect(await result.current.startRun(["image.png"])).toBe(false);
    });

    expect(result.current.draft).toBe("  Keep this draft  ");
    expect(result.current.runs[0].status).toBe("failed");
    expect(result.current.runs[0].instances).toEqual([]);
    expect(result.current.error).toBeTruthy();
    expect(result.current.isSubmitting).toBe(false);
  });

  it.each([false, true])("keeps the pending draft and suppresses duplicate starts, with newer edit=%s", async (editDraft) => {
    const options = createOptions();
    let finish!: (result: SendMessageResult) => void;
    options.sendUserMessageToThread.mockReturnValue(new Promise<SendMessageResult>((resolve) => { finish = resolve; }));
    const { result } = renderHook(() => useWorkspaceHome(options));
    act(() => result.current.setDraft("Original draft"));
    let pending!: Promise<boolean>;
    let duplicate!: Promise<boolean>;
    act(() => {
      pending = result.current.startRun();
      duplicate = result.current.startRun();
    });
    await expect(duplicate).resolves.toBe(false);
    await waitFor(() => expect(options.sendUserMessageToThread).toHaveBeenCalledOnce());
    expect(result.current.draft).toBe("Original draft");
    if (editDraft) act(() => result.current.setDraft("Next draft"));

    await act(async () => {
      finish({ status: "sent", threadId: "recovered-thread" });
      expect(await pending).toBe(true);
    });

    expect(result.current.draft).toBe(editDraft ? "Next draft" : "");
    expect(result.current.runs).toHaveLength(1);
    expect(result.current.runs[0].instances[0].threadId).toBe("recovered-thread");
    expect(result.current.isSubmitting).toBe(false);
  });

  it("counts only accepted worktree messages in a partially successful run", async () => {
    const options = createOptions();
    options.sendUserMessageToThread
      .mockResolvedValueOnce({ status: "sent", threadId: "recovered-thread" })
      .mockResolvedValueOnce({ status: "blocked" });
    const { result } = renderHook(() => useWorkspaceHome(options));
    act(() => {
      result.current.setRunMode("worktree");
      result.current.toggleModelSelection("id-1");
      result.current.setModelCount("id-1", 2);
      result.current.setDraft("Start both agents");
    });

    await act(async () => { expect(await result.current.startRun()).toBe(true); });

    expect(result.current.runs[0].status).toBe("partial");
    expect(result.current.runs[0].instances).toHaveLength(1);
    expect(result.current.runs[0].instances[0].threadId).toBe("recovered-thread");
    expect(result.current.runs[0].error).toBe("Started 1/2 runs. 1 failed.");
  });

  it("clears the originating workspace draft without changing a newly selected workspace", async () => {
    const options = createOptions();
    let finish!: (result: SendMessageResult) => void;
    options.sendUserMessageToThread.mockReturnValue(new Promise<SendMessageResult>((resolve) => { finish = resolve; }));
    const { result, rerender } = renderHook((activeWorkspace) => useWorkspaceHome({ ...options, activeWorkspace }), {
      initialProps: workspace,
    });
    act(() => result.current.setDraft("Original draft"));
    let pending!: Promise<boolean>;
    act(() => { pending = result.current.startRun(); });
    await waitFor(() => expect(options.sendUserMessageToThread).toHaveBeenCalledOnce());
    rerender({ ...workspace, id: "ws-2" });
    act(() => result.current.setDraft("Other workspace draft"));

    await act(async () => {
      finish({ status: "sent" });
      await pending;
    });

    expect(result.current.draft).toBe("Other workspace draft");
    rerender(workspace);
    expect(result.current.draft).toBe("");
  });

  it("uses provider model name for worktree runs", async () => {
    const addWorktreeAgent = vi.fn().mockResolvedValue(worktreeWorkspace);
    const connectWorkspace = vi.fn().mockResolvedValue(undefined);
    const startThreadForWorkspace = vi.fn().mockResolvedValue("thread-1");
    const sendUserMessageToThread = vi.fn().mockResolvedValue(undefined);
    const seedThreadCodexParams = vi.fn();
    vi.mocked(generateRunMetadata).mockResolvedValue({
      title: "Test run",
      worktreeName: "feat/test",
    });

    const { result } = renderHook(() =>
      useWorkspaceHome({
        activeWorkspace: workspace,
        models,
        selectedModelId: null,
        seedThreadCodexParams,
        addWorktreeAgent,
        connectWorkspace,
        startThreadForWorkspace,
        sendUserMessageToThread,
      }),
    );

    act(() => {
      result.current.setRunMode("worktree");
      result.current.toggleModelSelection("id-1");
      result.current.setDraft("Hello worktree");
    });

    await act(async () => {
      await result.current.startRun();
    });

    expect(sendUserMessageToThread).toHaveBeenCalledWith(
      worktreeWorkspace,
      "thread-1",
      "Hello worktree",
      [],
      expect.objectContaining({ model: "gpt-5.1-max" }),
    );
    expect(seedThreadCodexParams).toHaveBeenCalledWith("wt-1", "thread-1", {
      modelId: "id-1",
      effort: null,
      serviceTier: undefined,
    });
  });

  it("allows image-only local runs", async () => {
    const addWorktreeAgent = vi.fn();
    const connectWorkspace = vi.fn().mockResolvedValue(undefined);
    const startThreadForWorkspace = vi.fn().mockResolvedValue("thread-1");
    const sendUserMessageToThread = vi.fn().mockResolvedValue(undefined);
    const seedThreadCodexParams = vi.fn();
    vi.mocked(generateRunMetadata).mockResolvedValue({
      title: "Image run",
      worktreeName: "feat/image",
    });

    const { result } = renderHook(() =>
      useWorkspaceHome({
        activeWorkspace: workspace,
        models,
        selectedModelId: "id-1",
        seedThreadCodexParams,
        addWorktreeAgent,
        connectWorkspace,
        startThreadForWorkspace,
        sendUserMessageToThread,
      }),
    );

    await act(async () => {
      const started = await result.current.startRun(["img-1"]);
      expect(started).toBe(true);
    });

    expect(sendUserMessageToThread).toHaveBeenCalledWith(
      workspace,
      "thread-1",
      "",
      ["img-1"],
      expect.objectContaining({ model: "gpt-5.1-max" }),
    );
    expect(seedThreadCodexParams).toHaveBeenCalledWith("ws-1", "thread-1", {
      modelId: "id-1",
      effort: null,
      serviceTier: undefined,
    });
  });

  it("blocks worktree runs without model selections", async () => {
    const addWorktreeAgent = vi.fn();
    const connectWorkspace = vi.fn();
    const startThreadForWorkspace = vi.fn();
    const sendUserMessageToThread = vi.fn();
    vi.mocked(generateRunMetadata).mockResolvedValue({
      title: "Blocked",
      worktreeName: "feat/blocked",
    });

    const { result } = renderHook(() =>
      useWorkspaceHome({
        activeWorkspace: workspace,
        models,
        selectedModelId: null,
        addWorktreeAgent,
        connectWorkspace,
        startThreadForWorkspace,
        sendUserMessageToThread,
      }),
    );

    act(() => {
      result.current.setRunMode("worktree");
      result.current.setDraft("Hello");
    });

    let started = true;
    await act(async () => {
      started = await result.current.startRun();
    });

    expect(started).toBe(false);
    expect(result.current.error).toBe(
      "Select at least one model to run in a worktree.",
    );
    expect(result.current.runs).toHaveLength(0);
  });

  it("captures partial failures for multi-instance worktree runs", async () => {
    const addWorktreeAgent = vi
      .fn()
      .mockResolvedValueOnce(worktreeWorkspace)
      .mockResolvedValueOnce(null);
    const connectWorkspace = vi.fn().mockResolvedValue(undefined);
    const startThreadForWorkspace = vi.fn().mockResolvedValue("thread-1");
    const sendUserMessageToThread = vi.fn().mockResolvedValue(undefined);
    vi.mocked(generateRunMetadata).mockResolvedValue({
      title: "Partial",
      worktreeName: "feat/partial",
    });

    const { result } = renderHook(() =>
      useWorkspaceHome({
        activeWorkspace: workspace,
        models,
        selectedModelId: null,
        addWorktreeAgent,
        connectWorkspace,
        startThreadForWorkspace,
        sendUserMessageToThread,
      }),
    );

    act(() => {
      result.current.setRunMode("worktree");
      result.current.toggleModelSelection("id-1");
      result.current.setModelCount("id-1", 2);
      result.current.setDraft("Hello");
    });

    await act(async () => {
      await result.current.startRun();
    });

    expect(result.current.runs[0].status).toBe("partial");
    expect(result.current.runs[0].instanceErrors.length).toBeGreaterThan(0);
  });

  it("updates title after metadata resolves for local runs", async () => {
    const addWorktreeAgent = vi.fn();
    const connectWorkspace = vi.fn().mockResolvedValue(undefined);
    const startThreadForWorkspace = vi.fn().mockResolvedValue("thread-1");
    const sendUserMessageToThread = vi.fn().mockResolvedValue(undefined);
    let resolveMetadata: (value: { title: string; worktreeName: string }) => void =
      () => {};
    vi.mocked(generateRunMetadata).mockReturnValue(
      new Promise((resolve) => {
        resolveMetadata = resolve;
      }),
    );

    const { result } = renderHook(() =>
      useWorkspaceHome({
        activeWorkspace: workspace,
        models,
        selectedModelId: "id-1",
        addWorktreeAgent,
        connectWorkspace,
        startThreadForWorkspace,
        sendUserMessageToThread,
      }),
    );

    act(() => {
      result.current.setDraft("Local prompt");
    });

    await act(async () => {
      await result.current.startRun();
    });

    expect(result.current.runs[0].title).toBe("Local prompt");

    await act(async () => {
      resolveMetadata({ title: "Meta title", worktreeName: "feat/meta" });
      await Promise.resolve();
    });

    expect(result.current.runs[0].title).toBe("Meta title");
  });

  it("keeps attachments when worktree selection is missing", async () => {
    const addWorktreeAgent = vi.fn();
    const connectWorkspace = vi.fn();
    const startThreadForWorkspace = vi.fn();
    const sendUserMessageToThread = vi.fn();
    vi.mocked(generateRunMetadata).mockResolvedValue({
      title: "Blocked",
      worktreeName: "feat/blocked",
    });

    const { result } = renderHook(() =>
      useWorkspaceHome({
        activeWorkspace: workspace,
        models,
        selectedModelId: null,
        addWorktreeAgent,
        connectWorkspace,
        startThreadForWorkspace,
        sendUserMessageToThread,
      }),
    );

    act(() => {
      result.current.setRunMode("worktree");
    });

    let started = true;
    await act(async () => {
      started = await result.current.startRun(["img-1"]);
    });

    expect(started).toBe(false);
    expect(result.current.runs).toHaveLength(0);
    expect(result.current.error).toBe(
      "Select at least one model to run in a worktree.",
    );
  });
});
