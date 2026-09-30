import { useCallback, useEffect, useMemo, useRef, useState, type ComponentType } from "react";
import {
  Blocks,
  Bot,
  ExternalLink,
  Flame,
  MessageSquare,
  Plus,
  RefreshCw,
  Search,
  Sparkles,
  X,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Webview } from "@tauri-apps/api/webview";
import {
  bridgeImportFile,
  createSidebarBrowser,
  reloadSidebarBrowser,
  setSidebarBrowserBounds,
  setSidebarBrowserVisible,
} from "@services/tauri";
import {
  subscribeSidebarBrowserDownload,
  type SidebarBrowserDownload,
} from "@services/events";
import {
  attachAssistantWebview,
  closeAssistantTab,
  createAssistantTab,
  selectAssistantProvider,
  type AssistantBrowserTab,
  type AssistantProviderId,
} from "../assistantTabs";

type ProviderIcon = ComponentType<{ size?: number; color?: string; className?: string }>;

type AssistantProvider = {
  id: AssistantProviderId;
  label: string;
  tabLabel: string;
  url: string;
  icon: ProviderIcon;
  color: string;
};

const PROVIDERS: AssistantProvider[] = [
  { id: "web", label: "Google Search", tabLabel: "Web search", url: "https://www.google.com", icon: Search, color: "#4285F4" },
  { id: "chatgpt", label: "ChatGPT", tabLabel: "ChatGPT", url: "https://chatgpt.com", icon: MessageSquare, color: "#10A37F" },
  { id: "claude", label: "Anthropic Claude", tabLabel: "Claude", url: "https://claude.ai", icon: Sparkles, color: "#E56A54" },
  { id: "gemini", label: "Google Gemini", tabLabel: "Gemini", url: "https://gemini.google.com", icon: Sparkles, color: "#6E9EFF" },
  { id: "copilot", label: "Microsoft Copilot", tabLabel: "Copilot", url: "https://copilot.microsoft.com", icon: Blocks, color: "#62A8FF" },
  { id: "mistral", label: "Le Chat Mistral", tabLabel: "Mistral", url: "https://chat.mistral.ai", icon: Flame, color: "#F28C28" },
];

const SIDEBAR_RESIZE_GUTTER = 8;
let tabCounter = 0;
let webviewCounter = 0;

function nextTabId(): string {
  tabCounter += 1;
  return `assistant-tab-${tabCounter}`;
}

function nextWebviewLabel(): string {
  webviewCounter += 1;
  return `ai-chatbot-tab-${Date.now()}-${webviewCounter}`;
}

let persistedTabs: AssistantBrowserTab[] = [createAssistantTab(nextTabId())];
let persistedActiveTabId = persistedTabs[0].id;

type DownloadNotice = SidebarBrowserDownload & {
  state: "importing" | "imported" | "error";
};

type TabRuntime = {
  loading: boolean;
  error: string | null;
};

function providerById(id: AssistantProviderId | null): AssistantProvider | null {
  return PROVIDERS.find((provider) => provider.id === id) ?? null;
}

function getSidebarWebviewBounds(element: HTMLElement) {
  const panel = element.getBoundingClientRect();
  const sidebar = element.closest(".sidebar")?.getBoundingClientRect() ?? panel;
  const left = Math.max(panel.left, sidebar.left);
  const top = Math.max(panel.top, sidebar.top);
  const right = Math.min(panel.right, sidebar.right - SIDEBAR_RESIZE_GUTTER);
  const bottom = Math.min(panel.bottom, sidebar.bottom);
  return {
    x: Math.round(left),
    y: Math.round(top),
    width: Math.max(1, Math.round(right - left)),
    height: Math.max(1, Math.round(bottom - top)),
  };
}

function afterLayout(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  });
}

async function hideWebview(tab: AssistantBrowserTab | undefined): Promise<void> {
  if (!tab?.webviewLabel) return;
  const webview = await Webview.getByLabel(tab.webviewLabel);
  if (!webview) return;
  await setSidebarBrowserVisible(webview.label, false).catch(() => webview.hide());
}

export function AiWebView() {
  const [tabs, setTabs] = useState<AssistantBrowserTab[]>(persistedTabs);
  const [activeTabId, setActiveTabId] = useState(persistedActiveTabId);
  const [runtimeByTab, setRuntimeByTab] = useState<Record<string, TabRuntime>>({});
  const [downloadByTab, setDownloadByTab] = useState<Record<string, DownloadNotice>>({});
  const containerRef = useRef<HTMLDivElement>(null);
  const activeWebviewRef = useRef<Webview | null>(null);
  const observerRef = useRef<ResizeObserver | null>(null);
  const tabsRef = useRef(tabs);
  const activeTabIdRef = useRef(activeTabId);
  const creatingTabsRef = useRef(new Set<string>());
  const mountedRef = useRef(true);

  const activeTab = useMemo(
    () => tabs.find((tab) => tab.id === activeTabId) ?? tabs[0],
    [activeTabId, tabs],
  );
  const activeProvider = providerById(activeTab?.providerId ?? null);
  const activeRuntime = activeTab ? runtimeByTab[activeTab.id] : undefined;
  const activeDownload = activeTab ? downloadByTab[activeTab.id] : undefined;

  useEffect(() => {
    tabsRef.current = tabs;
    persistedTabs = tabs;
  }, [tabs]);

  useEffect(() => {
    activeTabIdRef.current = activeTabId;
    persistedActiveTabId = activeTabId;
  }, [activeTabId]);

  const updateRuntime = useCallback((tabId: string, patch: Partial<TabRuntime>) => {
    setRuntimeByTab((current) => ({
      ...current,
      [tabId]: { ...(current[tabId] ?? { loading: false, error: null }), ...patch },
    }));
  }, []);

  const syncWebviewBounds = useCallback(() => {
    const webview = activeWebviewRef.current;
    const element = containerRef.current;
    if (!webview || !element) return;
    void setSidebarBrowserBounds(webview.label, getSidebarWebviewBounds(element)).catch((error) => {
      updateRuntime(activeTabIdRef.current, { error: String(error) });
    });
  }, [updateRuntime]);

  const showTabWebview = useCallback(async (tab: AssistantBrowserTab, provider: AssistantProvider) => {
    if (!containerRef.current || creatingTabsRef.current.has(tab.id)) return;
    creatingTabsRef.current.add(tab.id);
    updateRuntime(tab.id, { loading: true, error: null });
    try {
      let webview = tab.webviewLabel ? await Webview.getByLabel(tab.webviewLabel) : null;
      let webviewLabel = tab.webviewLabel;
      if (!webview) {
        await afterLayout();
        const element = containerRef.current;
        if (!element) return;
        webviewLabel = nextWebviewLabel();
        await createSidebarBrowser(webviewLabel, provider.url, getSidebarWebviewBounds(element));
        webview = await Webview.getByLabel(webviewLabel);
        if (!webview) throw new Error("The assistant tab was created but could not be attached.");
        setTabs((current) => attachAssistantWebview(current, tab.id, webviewLabel!));
      }
      await afterLayout();
      const isActive = mountedRef.current && activeTabIdRef.current === tab.id;
      if (!isActive) {
        await setSidebarBrowserVisible(webview.label, false).catch(() => webview.hide());
        return;
      }
      activeWebviewRef.current = webview;
      syncWebviewBounds();
      await setSidebarBrowserVisible(webview.label, true).catch(() => webview.show());
      updateRuntime(tab.id, { loading: false, error: null });
    } catch (error) {
      updateRuntime(tab.id, { loading: false, error: String(error) });
    } finally {
      creatingTabsRef.current.delete(tab.id);
    }
  }, [syncWebviewBounds, updateRuntime]);

  useEffect(() => {
    if (!activeTab?.providerId || !activeProvider || !containerRef.current) {
      activeWebviewRef.current = null;
      return;
    }
    void showTabWebview(activeTab, activeProvider);
  }, [activeProvider, activeTab, showTabWebview]);

  useEffect(() => {
    mountedRef.current = true;
    window.addEventListener("resize", syncWebviewBounds);
    return () => {
      mountedRef.current = false;
      observerRef.current?.disconnect();
      observerRef.current = null;
      window.removeEventListener("resize", syncWebviewBounds);
      const tab = tabsRef.current.find((item) => item.id === activeTabIdRef.current);
      void hideWebview(tab);
      activeWebviewRef.current = null;
    };
  }, [syncWebviewBounds]);

  useEffect(() => {
    const element = containerRef.current;
    if (!activeProvider || !element) return;
    observerRef.current?.disconnect();
    const observer = new ResizeObserver(syncWebviewBounds);
    observer.observe(element);
    observerRef.current = observer;
    return () => observer.disconnect();
  }, [activeProvider, activeTabId, syncWebviewBounds]);

  useEffect(() => subscribeSidebarBrowserDownload((download) => {
    const tab = tabsRef.current.find((item) => item.webviewLabel === download.webviewLabel);
    if (!tab) return;
    setDownloadByTab((current) => ({
      ...current,
      [tab.id]: { ...download, state: download.imported ? "imported" : "error" },
    }));
  }, {
    onError: (error) => console.error("Failed to watch assistant downloads:", error),
  }), []);

  const activateTab = async (tabId: string) => {
    if (tabId === activeTabIdRef.current) return;
    const previous = tabsRef.current.find((tab) => tab.id === activeTabIdRef.current);
    activeTabIdRef.current = tabId;
    persistedActiveTabId = tabId;
    setActiveTabId(tabId);
    activeWebviewRef.current = null;
    await hideWebview(previous);
  };

  const addTab = () => {
    const tab = createAssistantTab(nextTabId());
    const previous = tabsRef.current.find((item) => item.id === activeTabIdRef.current);
    const nextTabs = [...tabsRef.current, tab];
    tabsRef.current = nextTabs;
    activeTabIdRef.current = tab.id;
    persistedActiveTabId = tab.id;
    setTabs(nextTabs);
    setActiveTabId(tab.id);
    activeWebviewRef.current = null;
    void hideWebview(previous);
  };

  const closeTab = (tabId: string) => {
    const currentTabs = tabsRef.current;
    const closingTab = currentTabs.find((tab) => tab.id === tabId);
    const result = closeAssistantTab(currentTabs, activeTabIdRef.current, tabId, nextTabId());
    tabsRef.current = result.tabs;
    activeTabIdRef.current = result.activeTabId;
    persistedActiveTabId = result.activeTabId;
    setTabs(result.tabs);
    setActiveTabId(result.activeTabId);
    if (closingTab?.webviewLabel) {
      void Webview.getByLabel(closingTab.webviewLabel).then((webview) => webview?.close()).catch(() => {});
    }
    if (tabId === activeTabId) activeWebviewRef.current = null;
    setRuntimeByTab((current) => {
      const next = { ...current };
      delete next[tabId];
      return next;
    });
    setDownloadByTab((current) => {
      const next = { ...current };
      delete next[tabId];
      return next;
    });
  };

  const chooseProvider = (providerId: AssistantProviderId) => {
    if (!activeTab) return;
    updateRuntime(activeTab.id, { loading: true, error: null });
    setTabs((current) => selectAssistantProvider(current, activeTab.id, providerId));
  };

  const reloadActiveTab = async () => {
    if (!activeTab?.webviewLabel) return;
    updateRuntime(activeTab.id, { error: null });
    try {
      await reloadSidebarBrowser(activeTab.webviewLabel);
    } catch (error) {
      updateRuntime(activeTab.id, { error: String(error) });
    }
  };

  const retryImport = async () => {
    if (!activeTab || !activeDownload?.path) return;
    setDownloadByTab((current) => ({
      ...current,
      [activeTab.id]: { ...activeDownload, state: "importing", error: null },
    }));
    try {
      await bridgeImportFile(activeDownload.path);
      setDownloadByTab((current) => ({
        ...current,
        [activeTab.id]: { ...activeDownload, state: "imported", error: null, imported: true },
      }));
    } catch (error) {
      setDownloadByTab((current) => ({
        ...current,
        [activeTab.id]: { ...activeDownload, state: "error", error: String(error) },
      }));
    }
  };

  return (
    <div className="ai-browser">
      <div className="ai-browser-tabbar">
        <div className="ai-browser-tabs" role="tablist" aria-label="Assistant tabs">
          {tabs.map((tab) => {
            const provider = providerById(tab.providerId);
            const Icon = provider?.icon ?? Bot;
            const isActive = tab.id === activeTabId;
            return (
              <div key={tab.id} className={`ai-browser-tab${isActive ? " is-active" : ""}`}>
                <button
                  className="ai-browser-tab-select"
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  onClick={() => void activateTab(tab.id)}
                >
                  <Icon size={14} color={provider?.color} />
                  <span>{provider?.tabLabel ?? "New tab"}</span>
                </button>
                <button className="ai-browser-tab-close" type="button" onClick={() => closeTab(tab.id)} aria-label={`Close ${provider?.tabLabel ?? "new"} tab`}>
                  <X size={12} />
                </button>
              </div>
            );
          })}
        </div>
        <button className="ai-browser-new-tab" type="button" onClick={addTab} aria-label="New assistant tab" title="New tab">
          <Plus size={16} />
        </button>
      </div>

      {activeProvider && (
        <div className="ai-browser-toolbar">
          <div className="ai-browser-location">
            <span style={{ background: activeProvider.color }} />
            <strong>{activeProvider.label}</strong>
            <small>{new URL(activeProvider.url).hostname}</small>
          </div>
          <div className="ai-browser-actions">
            <button type="button" onClick={() => void reloadActiveTab()} aria-label="Reload tab" title="Reload"><RefreshCw size={14} /></button>
            <button type="button" onClick={() => void openUrl(activeProvider.url)} aria-label="Open in external browser" title="Open in browser"><ExternalLink size={14} /></button>
          </div>
        </div>
      )}

      {activeDownload && (
        <div className={`ai-webview-download is-${activeDownload.state}`} role="status">
          <div>
            <strong>{activeDownload.fileName || "Assistant download"}</strong>
            {activeDownload.state === "importing" && <span>Adding to Bridge…</span>}
            {activeDownload.state === "imported" && <span>Added to Bridge Inbox.</span>}
            {activeDownload.state === "error" && <span>{activeDownload.error}</span>}
          </div>
          {activeDownload.state === "error" && activeDownload.path && <button type="button" onClick={() => void retryImport()}>Try again</button>}
          <button className="ai-webview-download-dismiss" type="button" onClick={() => {
            if (!activeTab) return;
            setDownloadByTab((current) => {
              const next = { ...current };
              delete next[activeTab.id];
              return next;
            });
          }} aria-label="Dismiss download notice"><X size={13} /></button>
        </div>
      )}

      <div className="ai-browser-content">
        {!activeProvider ? (
          <div className="ai-browser-picker">
            <div className="ai-browser-picker-heading">
              <div className="ai-browser-picker-mark"><Plus size={20} /></div>
              <h2>Open a tab</h2>
              <p>Search the web or start an assistant conversation. Each tab keeps its own session.</p>
            </div>
            <div className="ai-browser-provider-grid">
              {PROVIDERS.map((provider) => {
                const Icon = provider.icon;
                return (
                  <button
                    key={provider.id}
                    type="button"
                    aria-label={`Open ${provider.tabLabel}`}
                    onClick={() => chooseProvider(provider.id)}
                  >
                    <span className="ai-browser-provider-icon" style={{ color: provider.color, borderColor: `${provider.color}55` }}><Icon size={19} /></span>
                    <span><strong>{provider.tabLabel}</strong><small>{new URL(provider.url).hostname}</small></span>
                  </button>
                );
              })}
            </div>
          </div>
        ) : (
          <div ref={containerRef} className="ai-browser-webview-host">
            {activeRuntime?.loading && !activeRuntime.error && <div className="ai-webview-status">Opening {activeProvider.tabLabel}…</div>}
            {activeRuntime?.error && (
              <div className="ai-webview-error">
                <strong>Couldn’t open this tab</strong>
                <p className="ai-webview-error-detail">{activeRuntime.error}</p>
                <button type="button" className="secondary" onClick={() => void openUrl(activeProvider.url)}>Open in your browser</button>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
