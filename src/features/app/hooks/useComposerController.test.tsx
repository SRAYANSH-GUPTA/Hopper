// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useComposerController } from "./useComposerController";

vi.mock("../../composer/hooks/useComposerImages", () => ({
  useComposerImages: () => ({}),
}));
vi.mock("../../threads/hooks/useQueuedSend", () => ({
  useQueuedSend: () => ({}),
}));
vi.mock("@services/events", () => ({
  subscribeWebChatFileImported: () => () => {},
}));
vi.mock("@/features/web-chat/webChatFiles", () => ({
  subscribeComposerFileOffers: () => () => {},
}));

function useDraftController(activeThreadId: string | null) {
  return useComposerController({
    activeThreadId,
    activeTurnId: null,
    activeWorkspaceId: "workspace-1",
    activeWorkspace: null,
    isProcessing: false,
    isReviewing: false,
    steerEnabled: false,
    followUpMessageBehavior: "queue",
    appsEnabled: false,
    provider: "codex",
    connectWorkspace: vi.fn(),
    startThreadForWorkspace: vi.fn(),
    sendUserMessage: vi.fn(),
    sendUserMessageToThread: vi.fn(),
    startFork: vi.fn(),
    startReview: vi.fn(),
    startResume: vi.fn(),
    startCompact: vi.fn(),
    startApps: vi.fn(),
    startMcp: vi.fn(),
    startModels: vi.fn(),
    startFast: vi.fn(),
    startStatus: vi.fn(),
    startUsage: vi.fn(),
  });
}

describe("composer draft acceptance", () => {
  it.each(["thread-1", null])("clears only the submitted draft after switching from %s", (threadId) => {
    const { result, rerender } = renderHook(useDraftController, { initialProps: threadId });
    act(() => result.current.handleDraftChange("Submitted message"));
    const acceptOriginalDraft = result.current.handleClearDraftIfUnchanged;
    rerender("thread-2");
    act(() => result.current.handleDraftChange("Other thread draft"));

    act(() => acceptOriginalDraft("Submitted message"));

    expect(result.current.activeDraft).toBe("Other thread draft");
    rerender(threadId);
    expect(result.current.activeDraft).toBe("");
  });

  it("preserves newer saved text when a previous send is accepted", () => {
    const { result, rerender } = renderHook(useDraftController, { initialProps: "thread-1" });
    act(() => result.current.handleDraftChange("Submitted message"));
    const acceptOriginalDraft = result.current.handleClearDraftIfUnchanged;
    act(() => result.current.handleDraftChange("Newer draft"));
    rerender("thread-2");

    act(() => acceptOriginalDraft("Submitted message"));

    rerender("thread-1");
    expect(result.current.activeDraft).toBe("Newer draft");
  });
});
