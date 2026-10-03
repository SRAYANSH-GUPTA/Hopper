// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RequestUserInputRequest } from "../../../types";
import { RequestUserInputMessage } from "./RequestUserInputMessage";

afterEach(() => cleanup());

function request(overrides: Partial<RequestUserInputRequest["params"]> = {}): RequestUserInputRequest {
  return {
    workspace_id: "ws-1",
    request_id: "claude-q-1",
    params: {
      thread_id: "thread-1",
      turn_id: "turn-1",
      item_id: "tool-1",
      questions: [
        {
          id: "q0",
          header: "Targets",
          question: "Which platforms?",
          multiSelect: true,
          options: [
            { label: "Linux", description: "" },
            { label: "macOS", description: "" },
            { label: "Windows", description: "" },
          ],
        },
        {
          id: "q1",
          header: "Format",
          question: "Which format?",
          options: [
            { label: "JSON", description: "" },
            { label: "YAML", description: "" },
          ],
        },
      ],
      ...overrides,
    },
  };
}

describe("RequestUserInputMessage", () => {
  it("collects several choices for multi-select questions and one for single-select", () => {
    const onSubmit = vi.fn();
    const pending = request();
    render(
      <RequestUserInputMessage requests={[pending]} activeThreadId="thread-1" onSubmit={onSubmit} />,
    );
    expect(screen.getByText("Select all that apply")).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox", { name: /Windows/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Linux/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Windows/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /macOS/ }));
    fireEvent.click(screen.getByRole("radio", { name: /JSON/ }));
    fireEvent.click(screen.getByRole("radio", { name: /YAML/ }));
    fireEvent.click(screen.getByRole("button", { name: "Submit" }));
    expect(onSubmit).toHaveBeenCalledWith(pending, {
      answers: {
        q0: { answers: ["Linux", "macOS"] },
        q1: { answers: ["YAML"] },
      },
    });
  });

  it("explains follow-up answers for agents that can't pause", () => {
    const onSubmit = vi.fn();
    render(
      <RequestUserInputMessage
        requests={[request({ answer_mode: "followUp" })]}
        activeThreadId="thread-1"
        onSubmit={onSubmit}
      />,
    );
    expect(screen.getByText(/your answer is sent as your next message/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Send answer" }));
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("only shows requests for the active thread", () => {
    render(
      <RequestUserInputMessage requests={[request()]} activeThreadId="thread-2" onSubmit={vi.fn()} />,
    );
    expect(screen.queryByText("Which platforms?")).toBeNull();
  });
});
