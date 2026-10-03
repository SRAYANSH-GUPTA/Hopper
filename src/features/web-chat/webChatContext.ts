// One-click transfer between embedded web assistant chats and Hopper threads.
//
// Web → Hopper: a captured conversation is staged on a thread and prepended to
// the next message sent there. Hopper → web: a thread's messages are formatted
// for pasting into the web chat's input box.
import { useCallback, useSyncExternalStore } from "react";
import type { ConversationItem } from "@/types";
import type { WebChatConversation } from "@services/tauri";
import { stripHandoffPrefix } from "@utils/threadItems.conversion";

export const WEB_CHAT_CONTEXT_HEADING = "## Web Chat Context";
export const WEB_CHAT_CONTEXT_MAX_CHARS = 200_000;
export const WEB_CHAT_USER_MARKER = "\n\n---\n\n**User:** ";

export type StagedWebChatContext = {
  provider: string;
  title: string | null;
  url: string;
  messageCount: number;
  prompt: string;
};

const staged = new Map<string, StagedWebChatContext>();
const listeners = new Set<() => void>();

function stagingKey(workspaceId: string, threadId: string): string {
  return `${workspaceId}:${threadId}`;
}

function notify() {
  for (const listener of listeners) listener();
}

function roleLabel(role: string): string {
  return role === "user" ? "User" : "Assistant";
}

/**
 * Builds the context block for the receiving agent. Conversations over the
 * budget drop their oldest messages so the most recent context survives.
 */
export function buildWebChatContextPrompt(
  conversation: WebChatConversation,
  maxChars = WEB_CHAT_CONTEXT_MAX_CHARS,
): string {
  const titlePart = conversation.title ? `: "${conversation.title}"` : "";
  const header = [
    WEB_CHAT_CONTEXT_HEADING,
    "",
    `Imported from ${conversation.provider} web chat${titlePart} (${conversation.url})`,
    `The conversation below happened in ${conversation.provider} on the web, outside Hopper.`,
    "Treat it as background and continue from it. The user's new message follows the transcript.",
    "",
    "### Conversation (oldest → newest)",
    "",
  ];

  const blocks: string[] = [];
  let remaining = maxChars;
  for (let index = conversation.messages.length - 1; index >= 0; index -= 1) {
    const message = conversation.messages[index];
    let block = `**${roleLabel(message.role)}:**\n${message.content}`;
    if (block.length > remaining) {
      if (blocks.length > 0) break;
      block = `${block.slice(0, remaining)}\n…(message truncated)`;
    }
    blocks.unshift(block);
    remaining -= block.length;
  }
  const omitted = conversation.messages.length - blocks.length;
  if (omitted > 0) {
    blocks.unshift(`_(${omitted} earlier ${omitted === 1 ? "message" : "messages"} omitted)_`);
  }
  return [...header, blocks.join("\n\n"), "", "_End of web chat context._"].join("\n");
}

/** Formats a Hopper thread's user and assistant messages for a web chat. */
export function buildWebChatExportText(items: ConversationItem[]): string {
  const blocks = items.flatMap((item) => {
    if (item.kind !== "message") return [];
    const text = stripHandoffPrefix(item.text).trim();
    return text ? [`**${roleLabel(item.role)}:**\n${text}`] : [];
  });
  if (blocks.length === 0) return "";
  return ["Continuing from my Hopper session:", "", blocks.join("\n\n")].join("\n");
}

export function stageWebChatContext(
  workspaceId: string,
  threadId: string,
  conversation: WebChatConversation,
): StagedWebChatContext {
  const context: StagedWebChatContext = {
    provider: conversation.provider,
    title: conversation.title,
    url: conversation.url,
    messageCount: conversation.messages.length,
    prompt: buildWebChatContextPrompt(conversation),
  };
  staged.set(stagingKey(workspaceId, threadId), context);
  notify();
  return context;
}

export function peekWebChatContext(
  workspaceId: string,
  threadId: string,
): StagedWebChatContext | null {
  return staged.get(stagingKey(workspaceId, threadId)) ?? null;
}

/** Returns and removes the staged context prompt for a thread. */
export function consumeWebChatContext(workspaceId: string, threadId: string): string | null {
  const key = stagingKey(workspaceId, threadId);
  const context = staged.get(key);
  if (!context) return null;
  staged.delete(key);
  notify();
  return context.prompt;
}

export function clearWebChatContext(workspaceId: string, threadId: string): void {
  if (staged.delete(stagingKey(workspaceId, threadId))) notify();
}

export function subscribeWebChatContext(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useStagedWebChatContext(
  workspaceId: string | null,
  threadId: string | null,
): StagedWebChatContext | null {
  const getSnapshot = useCallback(
    () => (workspaceId && threadId ? peekWebChatContext(workspaceId, threadId) : null),
    [workspaceId, threadId],
  );
  return useSyncExternalStore(subscribeWebChatContext, getSnapshot, getSnapshot);
}
