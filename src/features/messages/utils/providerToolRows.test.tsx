// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ConversationItem, ThreadTokenUsage } from "../../../types";
import { buildConversationItem } from "../../../utils/threadItems";
import { WorkingIndicator } from "../components/MessageRows";
import { buildContextMeter } from "../../composer/components/ComposerMetaBar";
import { buildToolSummary, formatTokenCount } from "./messageRenderUtils";

afterEach(() => cleanup());

function usage(output: number, last = 50_000, window: number | null = 200_000): ThreadTokenUsage {
  const breakdown = (total: number, out: number) => ({
    totalTokens: total,
    inputTokens: total - out,
    cachedInputTokens: 0,
    outputTokens: out,
    reasoningOutputTokens: 0,
  });
  return { total: breakdown(output * 10, output), last: breakdown(last, 100), modelContextWindow: window };
}

describe("provider tool rows", () => {
  it("renders Claude/agy tool calls with their own title and target", () => {
    const item = buildConversationItem({
      type: "toolCall",
      id: "tool-1",
      tool: "Read",
      title: "Read",
      detail: "/repo/src/app.ts",
      status: "completed",
      output: "file body",
    }) as Extract<ConversationItem, { kind: "tool" }>;
    expect(item).toMatchObject({ kind: "tool", toolType: "toolCall", title: "Read", output: "file body" });
    expect(buildToolSummary(item, "")).toMatchObject({
      label: "read",
      value: "app.ts",
      detail: "/repo/src/app.ts",
    });
    const search = { ...item, title: "Search", detail: "TODO in src", status: "inProgress" };
    expect(buildToolSummary(search, "")).toMatchObject({ label: "searching", value: "TODO in src" });
    const agent = { ...item, title: "Agent", detail: "Explore the API" };
    expect(buildToolSummary(agent, "")).toMatchObject({ label: "agent", value: "Explore the API" });
  });
});

describe("token usage display", () => {
  it("formats token counts compactly", () => {
    expect(formatTokenCount(950)).toBe("950");
    expect(formatTokenCount(1234)).toBe("1.2k");
    expect(formatTokenCount(2000)).toBe("2k");
    expect(formatTokenCount(2_500_000)).toBe("2.5M");
  });

  it("shows only the tokens generated during the current turn", () => {
    const { rerender } = render(
      <WorkingIndicator isThinking={false} hasItems tokenUsage={usage(5000)} />,
    );
    rerender(<WorkingIndicator isThinking hasItems tokenUsage={usage(5000)} />);
    expect(screen.queryByText(/tokens$/)).toBeNull();
    rerender(<WorkingIndicator isThinking hasItems tokenUsage={usage(6234)} />);
    expect(screen.getByText("↓ 1.2k tokens")).toBeTruthy();
  });

  it("measures how full the context window is", () => {
    expect(buildContextMeter(usage(100, 50_000))).toMatchObject({ percent: 25 });
    expect(buildContextMeter(usage(100, 190_000))?.percent).toBe(95);
    expect(buildContextMeter(usage(100, 50_000))?.title).toContain("50k of 200k context tokens used");
    expect(buildContextMeter(usage(100, 50_000, null))).toBeNull();
    expect(buildContextMeter(null)).toBeNull();
  });
});
