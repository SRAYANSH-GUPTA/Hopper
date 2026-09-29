export type AssistantProviderId = "claude" | "chatgpt" | "copilot" | "gemini" | "mistral";

export type AssistantBrowserTab = {
  id: string;
  providerId: AssistantProviderId | null;
  webviewLabel: string | null;
};

export function createAssistantTab(id: string): AssistantBrowserTab {
  return { id, providerId: null, webviewLabel: null };
}

export function selectAssistantProvider(
  tabs: AssistantBrowserTab[],
  tabId: string,
  providerId: AssistantProviderId,
): AssistantBrowserTab[] {
  return tabs.map((tab) => tab.id === tabId ? { ...tab, providerId } : tab);
}

export function attachAssistantWebview(
  tabs: AssistantBrowserTab[],
  tabId: string,
  webviewLabel: string,
): AssistantBrowserTab[] {
  return tabs.map((tab) => tab.id === tabId ? { ...tab, webviewLabel } : tab);
}

export function closeAssistantTab(
  tabs: AssistantBrowserTab[],
  activeTabId: string,
  closingTabId: string,
  replacementId: string,
): { tabs: AssistantBrowserTab[]; activeTabId: string } {
  const closingIndex = tabs.findIndex((tab) => tab.id === closingTabId);
  const remaining = tabs.filter((tab) => tab.id !== closingTabId);
  if (remaining.length === 0) {
    return { tabs: [createAssistantTab(replacementId)], activeTabId: replacementId };
  }
  if (activeTabId !== closingTabId) {
    return { tabs: remaining, activeTabId };
  }
  const nextIndex = Math.min(Math.max(closingIndex, 0), remaining.length - 1);
  return { tabs: remaining, activeTabId: remaining[nextIndex].id };
}
