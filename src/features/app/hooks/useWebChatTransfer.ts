import { useCallback, useMemo, useRef } from "react";
import type { ConversationItem, WorkspaceInfo } from "@/types";
import { captureWebChat, insertIntoWebChat } from "@services/tauri";
import type { AssistantTransfer } from "@/features/ai-web/components/AiWebView";
import {
  buildWebChatExportText,
  clearWebChatContext,
  stageWebChatContext,
  useStagedWebChatContext,
} from "@/features/web-chat/webChatContext";

export type StagedContextChip = {
  label: string;
  title: string;
  onRemove: () => void;
};

type UseWebChatTransferOptions = {
  activeWorkspace: WorkspaceInfo | null;
  activeThreadId: string | null;
  activeItems: ConversationItem[];
  startThreadForWorkspace: (
    workspaceId: string,
    options?: { activate?: boolean },
  ) => Promise<string | null>;
};

/** Moves conversations between embedded assistant tabs and the active Hopper chat. */
export function useWebChatTransfer(options: UseWebChatTransferOptions): {
  transfer: AssistantTransfer;
  contextChip: StagedContextChip | null;
} {
  // Read through a ref so the handlers stay stable while the thread streams.
  const latest = useRef(options);
  latest.current = options;

  const sendToHopper = useCallback(async (label: string) => {
    const { activeWorkspace, activeThreadId, startThreadForWorkspace } = latest.current;
    if (!activeWorkspace) throw new Error("Open a workspace in Hopper first.");
    const conversation = await captureWebChat(label);
    const threadId = activeThreadId
      ?? await startThreadForWorkspace(activeWorkspace.id, { activate: true });
    if (!threadId) throw new Error("Couldn't open a Hopper chat for this conversation.");
    stageWebChatContext(activeWorkspace.id, threadId, conversation);
    const count = conversation.messages.length;
    return `${count} ${count === 1 ? "message" : "messages"} from ${conversation.provider} will be sent with your next Hopper message.`;
  }, []);

  const pasteHopperChat = useCallback(async (label: string) => {
    const text = buildWebChatExportText(latest.current.activeItems);
    if (!text) throw new Error("The active Hopper chat has no messages yet.");
    await insertIntoWebChat(label, text);
    return "Hopper chat pasted. Review it, then press send.";
  }, []);

  const transfer = useMemo(() => ({ sendToHopper, pasteHopperChat }), [sendToHopper, pasteHopperChat]);

  const workspaceId = options.activeWorkspace?.id ?? null;
  const threadId = options.activeThreadId;
  const staged = useStagedWebChatContext(workspaceId, threadId);
  const contextChip = useMemo(() => {
    if (!staged || !workspaceId || !threadId) return null;
    const count = staged.messageCount;
    return {
      label: `${staged.provider} chat · ${count} ${count === 1 ? "message" : "messages"}`,
      title: staged.title ? `${staged.title}\n${staged.url}` : staged.url,
      onRemove: () => clearWebChatContext(workspaceId, threadId),
    };
  }, [staged, workspaceId, threadId]);

  return { transfer, contextChip };
}
