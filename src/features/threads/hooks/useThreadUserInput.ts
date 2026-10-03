import { useCallback } from "react";
import type { Dispatch } from "react";
import type {
  ConversationItem,
  RequestUserInputRequest,
  RequestUserInputResponse,
  WorkspaceInfo,
} from "@/types";
import { respondToUserInputRequest } from "@services/tauri";
import type { ThreadAction } from "./useThreadsReducer";

type UseThreadUserInputOptions = {
  dispatch: Dispatch<ThreadAction>;
  /** Needed for "followUp" requests, whose answers are sent as a chat message. */
  activeWorkspace?: WorkspaceInfo | null;
  sendUserMessageToThread?: (
    workspace: WorkspaceInfo,
    threadId: string,
    text: string,
  ) => Promise<unknown>;
};

function asString(value: unknown) {
  return typeof value === "string" ? value : value ? String(value) : "";
}

function buildUserInputConversationItem(
  request: RequestUserInputRequest,
  response: RequestUserInputResponse,
): Extract<ConversationItem, { kind: "userInput" }> {
  const threadId = asString(request.params.thread_id).trim();
  const turnId = asString(request.params.turn_id).trim();
  const itemId = asString(request.params.item_id).trim();
  const requestId = asString(request.request_id).trim();
  const answered = response.answers ?? {};
  const seen = new Set<string>();
  const questions = request.params.questions.map((question, index) => {
    const id = question.id || `question-${index + 1}`;
    seen.add(id);
    const record = answered[id];
    const answers = Array.isArray(record?.answers)
      ? record.answers.map((entry) => asString(entry).trim()).filter(Boolean)
      : [];
    return {
      id,
      header: asString(question.header).trim(),
      question: asString(question.question).trim(),
      answers,
    };
  });
  const extra = Object.entries(answered)
    .filter(([id]) => !seen.has(id))
    .map(([id, value]) => {
      const answers = Array.isArray(value?.answers)
        ? value.answers.map((entry) => asString(entry).trim()).filter(Boolean)
        : [];
      return {
        id,
        header: "",
        question: id,
        answers,
      };
    });
  const entries = [...questions, ...extra];
  if (!entries.length) {
    entries.push({
      id: "user-input",
      header: "",
      question: "Input requested",
      answers: [],
    });
  }
  return {
    id: itemId
      ? [
          "user-input",
          request.workspace_id,
          threadId || "thread",
          turnId || "turn",
          itemId,
        ].join("-")
      : [
          "user-input",
          request.workspace_id,
          threadId || "thread",
          turnId || "turn",
          `request-${requestId || "unknown"}`,
        ].join("-"),
    kind: "userInput",
    status: "answered",
    questions: entries,
  };
}

/** Formats answers as a chat message for agents that can't pause for input. */
export function buildFollowUpAnswerMessage(
  request: RequestUserInputRequest,
  response: RequestUserInputResponse,
): string {
  const lines = request.params.questions.map((question, index) => {
    const id = question.id || `question-${index + 1}`;
    const answers = (response.answers?.[id]?.answers ?? [])
      .map((entry) => asString(entry).replace(/^user_note:\s*/, "").trim())
      .filter(Boolean);
    return `Q: ${asString(question.question).trim()}\nA: ${answers.length ? answers.join(", ") : "(no answer)"}`;
  });
  return ["Answers to your questions:", "", ...lines].join("\n");
}

export function useThreadUserInput({
  dispatch,
  activeWorkspace = null,
  sendUserMessageToThread,
}: UseThreadUserInputOptions) {
  const handleUserInputSubmit = useCallback(
    async (request: RequestUserInputRequest, response: RequestUserInputResponse) => {
      if (request.params.answer_mode === "followUp") {
        if (
          !sendUserMessageToThread ||
          !activeWorkspace ||
          activeWorkspace.id !== request.workspace_id
        ) {
          throw new Error("Open this chat's workspace to send your answer.");
        }
        await sendUserMessageToThread(
          activeWorkspace,
          request.params.thread_id,
          buildFollowUpAnswerMessage(request, response),
        );
      } else {
        await respondToUserInputRequest(
          request.workspace_id,
          request.request_id,
          response.answers,
        );
        // Follow-up answers already appear in the thread as the sent message.
        dispatch({
          type: "upsertItem",
          workspaceId: request.workspace_id,
          threadId: request.params.thread_id,
          item: buildUserInputConversationItem(request, response),
        });
      }
      dispatch({
        type: "removeUserInputRequest",
        requestId: request.request_id,
        workspaceId: request.workspace_id,
      });
    },
    [activeWorkspace, dispatch, sendUserMessageToThread],
  );

  return { handleUserInputSubmit };
}
