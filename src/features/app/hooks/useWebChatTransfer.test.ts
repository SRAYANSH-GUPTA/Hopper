/** @vitest-environment jsdom */
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ConversationItem, WorkspaceInfo } from "@/types";
import { captureWebChat, insertIntoWebChat } from "@services/tauri";
import {
  clearWebChatContext,
  peekWebChatContext,
} from "@/features/web-chat/webChatContext";
import { useWebChatTransfer } from "./useWebChatTransfer";

vi.mock("@services/tauri", () => ({
  captureWebChat: vi.fn(),
  insertIntoWebChat: vi.fn(),
}));

const workspace: WorkspaceInfo = {
  id: "ws-1",
  name: "Workspace",
  path: "/tmp/workspace",
  connected: true,
  settings: { sidebarCollapsed: false },
};

const conversation = {
  provider: "Claude",
  title: "Schema",
  url: "https://claude.ai/chat/1",
  messages: [
    { role: "user" as const, content: "Design the schema" },
    { role: "assistant" as const, content: "Here is a schema." },
  ],
};

function renderTransfer(overrides: Partial<Parameters<typeof useWebChatTransfer>[0]> = {}) {
  const startThreadForWorkspace = vi.fn(async () => "thread-new");
  const hook = renderHook(() =>
    useWebChatTransfer({
      activeWorkspace: workspace,
      activeThreadId: "thread-1",
      activeItems: [],
      startThreadForWorkspace,
      ...overrides,
    }),
  );
  return { ...hook, startThreadForWorkspace };
}

beforeEach(() => {
  vi.mocked(captureWebChat).mockResolvedValue(conversation);
  vi.mocked(insertIntoWebChat).mockResolvedValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
  clearWebChatContext("ws-1", "thread-1");
  clearWebChatContext("ws-1", "thread-new");
});

describe("useWebChatTransfer", () => {
  it("stages a captured web chat on the active thread and shows a chip", async () => {
    const { result, startThreadForWorkspace } = renderTransfer();
    let message = "";
    await act(async () => {
      message = await result.current.transfer.sendToHopper("ai-chatbot-tab-1");
    });
    expect(captureWebChat).toHaveBeenCalledWith("ai-chatbot-tab-1");
    expect(startThreadForWorkspace).not.toHaveBeenCalled();
    expect(message).toBe("2 messages from Claude will be sent with your next Hopper message.");
    expect(peekWebChatContext("ws-1", "thread-1")?.messageCount).toBe(2);
    expect(result.current.contextChip?.label).toBe("Claude chat · 2 messages");

    act(() => result.current.contextChip?.onRemove());
    expect(peekWebChatContext("ws-1", "thread-1")).toBeNull();
    expect(result.current.contextChip).toBeNull();
  });

  it("opens a thread when none is active", async () => {
    const { result, startThreadForWorkspace } = renderTransfer({ activeThreadId: null });
    await act(async () => {
      await result.current.transfer.sendToHopper("ai-chatbot-tab-1");
    });
    expect(startThreadForWorkspace).toHaveBeenCalledWith("ws-1", { activate: true });
    expect(peekWebChatContext("ws-1", "thread-new")).not.toBeNull();
  });

  it("requires an open workspace", async () => {
    const { result } = renderTransfer({ activeWorkspace: null });
    await expect(result.current.transfer.sendToHopper("ai-chatbot-tab-1")).rejects.toThrow(
      "Open a workspace in Hopper first.",
    );
    expect(captureWebChat).not.toHaveBeenCalled();
  });

  it("pastes the active Hopper chat into the web chat", async () => {
    const activeItems: ConversationItem[] = [
      { id: "1", kind: "message", role: "user", text: "Hi" },
      { id: "2", kind: "message", role: "assistant", text: "Hello" },
    ];
    const { result } = renderTransfer({ activeItems });
    await act(async () => {
      await result.current.transfer.pasteHopperChat("ai-chatbot-tab-1");
    });
    expect(insertIntoWebChat).toHaveBeenCalledWith(
      "ai-chatbot-tab-1",
      "Continuing from my Hopper session:\n\n**User:**\nHi\n\n**Assistant:**\nHello",
    );
  });

  it("refuses to paste an empty Hopper chat", async () => {
    const { result } = renderTransfer();
    await expect(result.current.transfer.pasteHopperChat("ai-chatbot-tab-1")).rejects.toThrow(
      "The active Hopper chat has no messages yet.",
    );
    expect(insertIntoWebChat).not.toHaveBeenCalled();
  });
});
