import { useEffect, useRef } from "react";
import { subscribeAppServerEvents } from "../../services/events";
import {
  appendTurn,
  buildHandoffPrompt,
  createThreadContext,
  loadThreadContext,
  saveThreadContext,
  savePendingHandoff,
  type TurnRecord,
} from "./contextStore";

/**
 * Listens to app-server-events and maintains a per-thread context snapshot
 * in localStorage. On provider switch, the caller requests a handoff prompt
 * via `triggerHandoff` which writes a pending handoff that the next sent
 * message will pick up and prepend automatically.
 */
export function useContextHandoff(activeProvider: string) {
  // workspace_id -> thread_id -> { userBuffer, assistantBuffer }
  const bufferRef = useRef<
    Record<string, Record<string, { user: string; assistant: string }>>
  >({});

  useEffect(() => {
    return subscribeAppServerEvents((event) => {
      const workspaceId = event.workspace_id;
      const msg = event.message as Record<string, unknown>;
      const method = msg.method as string | undefined;
      const params = (msg.params ?? {}) as Record<string, unknown>;
      const threadId = params.threadId as string | undefined;

      if (!method || !threadId) return;

      // ── Capture user message ───────────────────────────────────────────
      if (method === "item/completed") {
        const item = params.item as Record<string, unknown> | undefined;
        if (!item) return;
        const itemType = item.type as string | undefined;

        if (itemType === "userMessage") {
          const content = item.content as { type: string; text: string }[] | undefined;
          const text = content?.find((c) => c.type === "text")?.text ?? "";
          if (!text) return;
          const ws = (bufferRef.current[workspaceId] ??= {});
          ws[threadId] = { user: text, assistant: "" };
        }
      }

      // ── Capture assistant delta ────────────────────────────────────────
      if (method === "item/agentMessage/delta") {
        const delta = params.delta as string | undefined;
        if (!delta) return;
        const ws = (bufferRef.current[workspaceId] ??= {});
        const buf = (ws[threadId] ??= { user: "", assistant: "" });
        buf.assistant += delta;
      }

      // ── Finalise turn on turn/completed ───────────────────────────────
      if (method === "turn/completed") {
        const ws = bufferRef.current[workspaceId];
        const buf = ws?.[threadId];
        if (!buf || !buf.user) return;

        const turn: TurnRecord = {
          userText: buf.user,
          assistantText: buf.assistant,
          provider: activeProvider,
          timestamp: Date.now(),
        };

        // Load or create the context snapshot for this thread
        let ctx = loadThreadContext(workspaceId, threadId);
        if (!ctx) {
          ctx = createThreadContext(workspaceId, threadId, buf.user);
        }
        ctx = appendTurn(ctx, turn);
        saveThreadContext(ctx);

        // Clear buffer for this thread
        delete ws[threadId];
      }
    });
  }, [activeProvider]);
}

/**
 * Call this when the user switches providers. Reads the most recent context
 * snapshot for the given workspace+thread and writes a pending handoff prompt
 * to localStorage so the next message will carry the full context.
 */
export function triggerHandoff(
  workspaceId: string,
  activeThreadId: string | null | undefined,
  nextProvider: string,
): void {
  if (!activeThreadId) return;
  const ctx = loadThreadContext(workspaceId, activeThreadId);
  if (!ctx || ctx.recentTurns.length === 0) return;
  const prompt = buildHandoffPrompt(ctx, nextProvider);
  savePendingHandoff(workspaceId, prompt);
}
