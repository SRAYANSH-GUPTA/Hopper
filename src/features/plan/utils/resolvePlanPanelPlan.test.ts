import { describe, expect, it } from "vitest";
import type { ConversationItem, TurnPlan } from "@/types";
import { resolvePlanPanelPlan } from "./resolvePlanPanelPlan";

const planItem = (output: string): ConversationItem => ({
  id: "plan-1",
  kind: "tool",
  toolType: "plan",
  title: "Plan",
  detail: "completed",
  output,
});

describe("resolvePlanPanelPlan", () => {
  it("prefers a structured live plan", () => {
    const livePlan: TurnPlan = {
      turnId: "turn-1",
      explanation: "Live plan",
      steps: [{ step: "Implement", status: "inProgress" }],
    };

    expect(resolvePlanPanelPlan(livePlan, [planItem("Generated plan")])).toBe(livePlan);
  });

  it("uses the latest generated plan item when no live plan exists", () => {
    expect(resolvePlanPanelPlan(null, [planItem("## Proposed Plan\n\n- Implement")])).toEqual({
      turnId: "plan-1",
      explanation: "## Proposed Plan\n\n- Implement",
      steps: [],
    });
  });

  it("does not keep a generated plan active after a newer user message", () => {
    const items: ConversationItem[] = [
      planItem("Generated plan"),
      { id: "user-1", kind: "message", role: "user", text: "Implement it" },
    ];

    expect(resolvePlanPanelPlan(null, items)).toBeNull();
  });

  describe("shared plan responses", () => {
    const request: ConversationItem = {
      id: "request-1", kind: "message", role: "user", text: "/plan\n\nAdd search",
    };
    const reply: ConversationItem = {
      id: "reply-1", kind: "message", role: "assistant", text: "## Plan\n\n1. Add search",
    };

    it("shows the plan reply from stored conversation items", () => {
      expect(resolvePlanPanelPlan(null, [request, reply])).toEqual({
        turnId: request.id,
        explanation: reply.text,
        steps: [],
      });
    });

    it("updates with streamed text and prefers the latest reply over commentary", () => {
      const items: ConversationItem[] = [request, reply, {
        id: "final", kind: "message", role: "assistant", text: "## Revised",
      }];
      expect(resolvePlanPanelPlan(null, items)?.explanation).toBe("## Revised");
      items[2] = { ...reply, id: "final", text: "## Revised plan\n\n1. Test search" };
      expect(resolvePlanPanelPlan(null, items)?.explanation).toBe(
        "## Revised plan\n\n1. Test search",
      );
    });

    it("does not reuse a previous reply while a new plan request is waiting", () => {
      expect(resolvePlanPanelPlan(null, [request, reply, { ...request, id: "request-2" }]))
        .toBeNull();
    });

    it("clears after an ordinary request and does not classify its answer as a plan", () => {
      const next: ConversationItem = { ...request, id: "request-2", text: "Implement it" };
      expect(resolvePlanPanelPlan(null, [request, reply, next])).toBeNull();
      expect(resolvePlanPanelPlan(null, [request, reply, next, { ...reply, id: "reply-2" }]))
        .toBeNull();
    });

    it("ignores reasoning and tools as plan content", () => {
      expect(resolvePlanPanelPlan(null, [request, {
        id: "thinking", kind: "reasoning", summary: "Thinking", content: "Investigating",
      }])).toBeNull();
    });

    it("does not treat a different command starting with plan as a plan request", () => {
      expect(resolvePlanPanelPlan(null, [{ ...request, text: "/planet info" }, reply]))
        .toBeNull();
    });

    it("allows a new provider plan after an older native plan item", () => {
      expect(resolvePlanPanelPlan(null, [planItem("Old plan"), request, reply])?.explanation)
        .toBe(reply.text);
    });
  });

  it("does not infer plans from ordinary assistant messages", () => {
    expect(resolvePlanPanelPlan(null, [
      { id: "user", kind: "message", role: "user", text: "Build search" },
      { id: "reply", kind: "message", role: "assistant", text: "I'll investigate" },
    ])).toBeNull();
  });
});
