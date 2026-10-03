// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { respondToUserInputRequest } from "@services/tauri";
import { useThreadUserInput } from "./useThreadUserInput";

vi.mock("@services/tauri", () => ({
  respondToUserInputRequest: vi.fn().mockResolvedValue(undefined),
}));

describe("useThreadUserInput", () => {
  it("submits request-user-input answers and appends an answered item", async () => {
    const dispatch = vi.fn();
    const { result } = renderHook(() => useThreadUserInput({ dispatch }));
    const request = {
      workspace_id: "ws-1",
      request_id: "req-7",
      params: {
        thread_id: "thread-1",
        turn_id: "turn-1",
        item_id: "item-1",
        questions: [
          {
            id: "q-choice",
            header: "Pick",
            question: "Which option?",
            options: [
              { label: "A", description: "Option A" },
              { label: "B", description: "Option B" },
            ],
          },
        ],
      },
    };
    const response = {
      answers: {
        "q-choice": { answers: ["A", "user_note: with details"] },
      },
    };

    await act(async () => {
      await result.current.handleUserInputSubmit(request, response);
    });

    expect(respondToUserInputRequest).toHaveBeenCalledWith(
      "ws-1",
      "req-7",
      response.answers,
    );
    expect(dispatch).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        type: "upsertItem",
        workspaceId: "ws-1",
        threadId: "thread-1",
        item: expect.objectContaining({
          id: "user-input-ws-1-thread-1-turn-1-item-1",
          kind: "userInput",
          status: "answered",
        }),
      }),
    );
    expect(dispatch).toHaveBeenNthCalledWith(2, {
      type: "removeUserInputRequest",
      requestId: "req-7",
      workspaceId: "ws-1",
    });
  });

  it("sends follow-up answers as a chat message instead of replying to the turn", async () => {
    vi.mocked(respondToUserInputRequest).mockClear();
    const dispatch = vi.fn();
    const sendUserMessageToThread = vi.fn().mockResolvedValue(undefined);
    const workspace = {
      id: "ws-1",
      name: "W",
      path: "/tmp/w",
      connected: true,
      settings: { sidebarCollapsed: false },
    };
    const { result } = renderHook(() =>
      useThreadUserInput({ dispatch, activeWorkspace: workspace, sendUserMessageToThread }),
    );
    const request = {
      workspace_id: "ws-1",
      request_id: "agy-q-1",
      params: {
        thread_id: "thread-1",
        turn_id: "turn-1",
        item_id: "tool-74",
        answer_mode: "followUp" as const,
        questions: [{ id: "q0", header: "", question: "Which part?", multiSelect: true }],
      },
    };
    await act(async () => {
      await result.current.handleUserInputSubmit(request, {
        answers: { q0: { answers: ["Home", "Sidebar", "user_note: both"] } },
      });
    });
    expect(respondToUserInputRequest).not.toHaveBeenCalled();
    expect(sendUserMessageToThread).toHaveBeenCalledWith(
      workspace,
      "thread-1",
      "Answers to your questions:\n\nQ: Which part?\nA: Home, Sidebar, both",
    );
    expect(dispatch).toHaveBeenCalledWith({
      type: "removeUserInputRequest",
      requestId: "agy-q-1",
      workspaceId: "ws-1",
    });
  });
});
