import { describe, expect, it } from "vitest";
import {
  attachAssistantWebview,
  closeAssistantTab,
  createAssistantTab,
  selectAssistantProvider,
} from "./assistantTabs";

describe("assistant browser tabs", () => {
  it("keeps separate webviews for tabs using the same provider", () => {
    const tabs = [createAssistantTab("one"), createAssistantTab("two")];
    const selected = selectAssistantProvider(
      selectAssistantProvider(tabs, "one", "chatgpt"),
      "two",
      "chatgpt",
    );
    const attached = attachAssistantWebview(
      attachAssistantWebview(selected, "one", "ai-chatbot-one"),
      "two",
      "ai-chatbot-two",
    );

    expect(attached.map((tab) => tab.webviewLabel)).toEqual([
      "ai-chatbot-one",
      "ai-chatbot-two",
    ]);
  });

  it("activates the neighboring tab when the active tab closes", () => {
    const tabs = [createAssistantTab("one"), createAssistantTab("two"), createAssistantTab("three")];

    expect(closeAssistantTab(tabs, "two", "two", "replacement")).toEqual({
      tabs: [tabs[0], tabs[2]],
      activeTabId: "three",
    });
  });

  it("creates a blank replacement when the final tab closes", () => {
    expect(closeAssistantTab([createAssistantTab("one")], "one", "one", "fresh")).toEqual({
      tabs: [createAssistantTab("fresh")],
      activeTabId: "fresh",
    });
  });
});
