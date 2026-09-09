import type { ConversationItem, TurnPlan } from "@/types";

function hasPlanContent(plan: TurnPlan | null): plan is TurnPlan {
  return Boolean(plan && (plan.explanation || plan.steps.length > 0));
}

export function resolvePlanPanelPlan(
  livePlan: TurnPlan | null,
  items: ConversationItem[],
): TurnPlan | null {
  if (hasPlanContent(livePlan)) {
    return livePlan;
  }

  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item.kind !== "tool" || item.toolType !== "plan") {
      continue;
    }

    const output = (item.output ?? "").trim();
    if (!output) {
      return null;
    }

    const hasNewerUserMessage = items
      .slice(index + 1)
      .some((candidate) => candidate.kind === "message" && candidate.role === "user");
    if (hasNewerUserMessage) {
      break;
    }

    return {
      turnId: item.id,
      explanation: output,
      steps: [],
    };
  }

  // Claude and Antigravity return plan text as assistant messages. The recorded
  // request identifies the plan regardless of which provider is now selected.
  let response: Extract<ConversationItem, { kind: "message" }> | null = null;
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item.kind !== "message") {
      continue;
    }
    if (item.role === "user") {
      if (!/^\/plan(?:\s|$)/i.test(item.text.trimStart()) || !response) {
        return null;
      }
      return {
        turnId: item.id,
        explanation: response.text.trim(),
        steps: [],
      };
    }
    if (!response && item.text.trim()) {
      response = item;
    }
  }

  return null;
}
