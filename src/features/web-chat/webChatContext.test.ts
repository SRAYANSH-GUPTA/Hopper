import { afterEach, describe, expect, it } from "vitest";
import type { ConversationItem } from "@/types";
import type { WebChatConversation } from "@services/tauri";
import { buildConversationItem } from "@utils/threadItems";
import {
  WEB_CHAT_USER_MARKER,
  buildWebChatContextPrompt,
  buildWebChatExportText,
  clearWebChatContext,
  consumeWebChatContext,
  peekWebChatContext,
  stageWebChatContext,
  subscribeWebChatContext,
} from "./webChatContext";

const conversation: WebChatConversation = {
  provider: "ChatGPT",
  title: "API design",
  url: "https://chatgpt.com/c/abc",
  messages: [
    { role: "user", content: "How should we version the API?" },
    { role: "assistant", content: "Use a /v1 prefix and additive changes." },
  ],
};

afterEach(() => {
  clearWebChatContext("ws-1", "thread-1");
});

describe("buildWebChatContextPrompt", () => {
  it("includes the source and the full transcript in order", () => {
    const prompt = buildWebChatContextPrompt(conversation);
    expect(prompt.startsWith("## Web Chat Context")).toBe(true);
    expect(prompt).toContain('Imported from ChatGPT web chat: "API design" (https://chatgpt.com/c/abc)');
    const user = prompt.indexOf("**User:**\nHow should we version the API?");
    const assistant = prompt.indexOf("**Assistant:**\nUse a /v1 prefix and additive changes.");
    expect(user).toBeGreaterThan(-1);
    expect(assistant).toBeGreaterThan(user);
    expect(prompt).not.toContain("omitted");
  });

  it("drops the oldest messages when over budget", () => {
    const long: WebChatConversation = {
      ...conversation,
      messages: [
        { role: "user", content: "old ".repeat(50) },
        { role: "assistant", content: "middle ".repeat(5) },
        { role: "user", content: "newest" },
      ],
    };
    const prompt = buildWebChatContextPrompt(long, 80);
    expect(prompt).toContain("_(1 earlier message omitted)_");
    expect(prompt).not.toContain("old old");
    expect(prompt).toContain("middle");
    expect(prompt).toContain("newest");
  });

  it("keeps the newest message even when it alone exceeds the budget", () => {
    const prompt = buildWebChatContextPrompt(
      { ...conversation, messages: [{ role: "user", content: "x".repeat(500) }] },
      100,
    );
    expect(prompt).toContain("…(message truncated)");
    expect(prompt).not.toContain("omitted");
  });
});

describe("buildWebChatExportText", () => {
  it("exports only user and assistant messages", () => {
    const items: ConversationItem[] = [
      { id: "1", kind: "message", role: "user", text: "Fix the login bug" },
      { id: "2", kind: "reasoning", summary: "thinking", content: "" },
      {
        id: "3",
        kind: "tool",
        toolType: "commandExecution",
        title: "npm test",
        detail: "",
        output: "ok",
      },
      { id: "4", kind: "message", role: "assistant", text: "Fixed it in auth.ts." },
    ] as ConversationItem[];
    expect(buildWebChatExportText(items)).toBe(
      "Continuing from my Hopper session:\n\n**User:**\nFix the login bug\n\n**Assistant:**\nFixed it in auth.ts.",
    );
  });

  it("returns an empty string for a chat without messages", () => {
    expect(buildWebChatExportText([])).toBe("");
  });
});

describe("staged web chat context", () => {
  it("stages, peeks, and consumes once", () => {
    let notifications = 0;
    const unsubscribe = subscribeWebChatContext(() => {
      notifications += 1;
    });
    const staged = stageWebChatContext("ws-1", "thread-1", conversation);
    expect(staged.messageCount).toBe(2);
    expect(peekWebChatContext("ws-1", "thread-1")).toEqual(staged);
    expect(peekWebChatContext("ws-1", "thread-2")).toBeNull();
    expect(consumeWebChatContext("ws-1", "thread-1")).toBe(staged.prompt);
    expect(consumeWebChatContext("ws-1", "thread-1")).toBeNull();
    expect(notifications).toBe(2);
    unsubscribe();
  });

  it("clears staged context", () => {
    stageWebChatContext("ws-1", "thread-1", conversation);
    clearWebChatContext("ws-1", "thread-1");
    expect(peekWebChatContext("ws-1", "thread-1")).toBeNull();
  });

  it("renders a sent message without the transcript and tags its source", () => {
    const prompt = buildWebChatContextPrompt(conversation);
    const item = buildConversationItem({
      type: "userMessage",
      id: "msg-1",
      content: [{ type: "text", text: `${prompt}${WEB_CHAT_USER_MARKER}Summarize what we decided` }],
    });
    expect(item).toMatchObject({
      kind: "message",
      role: "user",
      text: "Summarize what we decided",
      handoffFrom: "ChatGPT (web)",
    });
  });
});
