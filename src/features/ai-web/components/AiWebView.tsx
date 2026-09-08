import { useState, useEffect, useRef, useCallback } from "react";
import { MessageSquare, ExternalLink, Sparkles, Flame, Blocks, RefreshCw } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Webview } from "@tauri-apps/api/webview";
import { setSidebarBrowserBounds } from "@services/tauri";

type Chatbot = "claude" | "chatgpt" | "copilot" | "gemini" | "mistral";

const CHATBOTS: { id: Chatbot; label: string; url: string; icon: React.ReactNode; color: string }[] = [
  { id: "claude", label: "Anthropic Claude", url: "https://claude.ai", icon: <Sparkles size={20} color="#E56A54" />, color: "#E56A54" },
  { id: "chatgpt", label: "ChatGPT", url: "https://chatgpt.com", icon: <MessageSquare size={20} color="#10A37F" />, color: "#10A37F" },
  { id: "copilot", label: "Copilot", url: "https://copilot.microsoft.com", icon: <Blocks size={20} color="#2A73E8" />, color: "#2A73E8" },
  { id: "gemini", label: "Google Gemini", url: "https://gemini.google.com", icon: <Sparkles size={20} color="#4A90E2" />, color: "#4A90E2" },
  { id: "mistral", label: "Le Chat Mistral", url: "https://chat.mistral.ai", icon: <Flame size={20} color="#E85D04" />, color: "#E85D04" },
];

let webviewCounter = 0;
const SIDEBAR_RESIZE_GUTTER = 8;

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

function getDesktopChromeUserAgent(): string {
  const hostUserAgent = navigator.userAgent;
  const platform = hostUserAgent.includes("Windows")
    ? "Windows NT 10.0; Win64; x64"
    : hostUserAgent.includes("Macintosh")
      ? "Macintosh; Intel Mac OS X 10_15_7"
      : "X11; Linux x86_64";
  return `Mozilla/5.0 (${platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36`;
}

export function AiWebView() {
  const [selectedChatbot, setSelectedChatbot] = useState<Chatbot | null>(null);
  const [activeChatbot, setActiveChatbot] = useState<Chatbot | null>(null);
  const [webviewError, setWebviewError] = useState<string | null>(null);
  const [isWebviewLoading, setIsWebviewLoading] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const webviewRef = useRef<Webview | null>(null);
  const readyWebviewRef = useRef<Webview | null>(null);
  const observerRef = useRef<ResizeObserver | null>(null);

  // Sync the webview position/size with the container element
  const syncWebviewBounds = useCallback(() => {
    const webview = webviewRef.current;
    const el = containerRef.current;
    if (!webview || !el || readyWebviewRef.current !== webview) return;

    const bounds = getSidebarWebviewBounds(el);
    void setSidebarBrowserBounds(webview.label, bounds).catch((error) => {
      setWebviewError(String(error));
    });
  }, []);

  // Create or destroy the native webview
  const openWebview = useCallback(async (bot: typeof CHATBOTS[number]) => {
    // Tear down old webview if any
    if (webviewRef.current) {
      try { await webviewRef.current.close(); } catch { /* already closed */ }
      webviewRef.current = null;
    }

    const el = containerRef.current;
    if (!el) return;

    try {
      setIsWebviewLoading(true);
      await afterLayout();
      if (containerRef.current !== el || !el.isConnected) return;
      const bounds = getSidebarWebviewBounds(el);
      const label = `ai-chatbot-${++webviewCounter}`;
      const appWindow = getCurrentWindow();
      const webview = new Webview(appWindow, label, {
        url: bot.url,
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
        userAgent: getDesktopChromeUserAgent(),
        backgroundColor: "#101310",
        transparent: false,
      });

      webviewRef.current = webview;
      readyWebviewRef.current = null;
      setWebviewError(null);

      void webview.once("tauri://created", () => {
        if (webviewRef.current !== webview) return;
        readyWebviewRef.current = webview;
        setIsWebviewLoading(false);
        syncWebviewBounds();
      });
      void webview.once<unknown>("tauri://error", (event) => {
        if (webviewRef.current !== webview) return;
        webviewRef.current = null;
        setIsWebviewLoading(false);
        const detail =
          typeof event.payload === "string"
            ? event.payload
            : JSON.stringify(event.payload);
        setWebviewError(detail || "The embedded browser could not be created.");
        console.error("Failed to create AI webview:", event.payload);
      });

      // Keep position in sync on resize / layout shifts
      if (observerRef.current) {
        observerRef.current.disconnect();
      }
      const observer = new ResizeObserver(() => syncWebviewBounds());
      observer.observe(el);
      observerRef.current = observer;

      // Also re-sync on window resize
      window.addEventListener("resize", syncWebviewBounds);
    } catch (err) {
      console.error("Failed to create webview:", err);
      setWebviewError(String(err));
      setIsWebviewLoading(false);
      // Fallback: open in external browser
      void openUrl(bot.url);
    }
  }, [syncWebviewBounds]);

  // Close webview on unmount
  useEffect(() => {
    return () => {
      if (webviewRef.current) {
        void webviewRef.current.close().catch(() => {});
        webviewRef.current = null;
      }
      if (observerRef.current) {
        observerRef.current.disconnect();
        observerRef.current = null;
      }
      window.removeEventListener("resize", syncWebviewBounds);
    };
  }, [syncWebviewBounds]);

  const handleContinue = () => {
    if (selectedChatbot) {
      setActiveChatbot(selectedChatbot);
    }
  };

  // The native webview needs the active panel's container to exist first.
  // Creating it directly in handleContinue runs before React mounts that node.
  useEffect(() => {
    if (!activeChatbot || webviewRef.current || !containerRef.current) {
      return;
    }
    const bot = CHATBOTS.find((candidate) => candidate.id === activeChatbot);
    if (bot) {
      void openWebview(bot);
    }
  }, [activeChatbot, openWebview]);

  const handleChange = async () => {
    if (webviewRef.current) {
      try { await webviewRef.current.close(); } catch { /* ok */ }
      webviewRef.current = null;
    }
    if (observerRef.current) {
      observerRef.current.disconnect();
      observerRef.current = null;
    }
    setActiveChatbot(null);
    setWebviewError(null);
    setIsWebviewLoading(false);
  };

  const handleRefresh = () => {
    const bot = CHATBOTS.find((b) => b.id === activeChatbot);
    if (bot) {
      void openWebview(bot);
    }
  };

  const activeBot = CHATBOTS.find((b) => b.id === activeChatbot);

  // ── Active webview panel ──
  if (activeChatbot && activeBot) {
    return (
      <div className="mcp-view" style={{ height: "100%", display: "flex", flexDirection: "column", background: "var(--bg-base, #111)" }}>
        {/* Header */}
        <div style={{
          padding: "8px 12px",
          borderBottom: "1px solid var(--border, #333)",
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          zIndex: 10,
          position: "relative",
        }}>
          <div style={{ display: "flex", alignItems: "center", gap: "8px", fontSize: "12px", fontWeight: 600, color: "var(--fg, #fff)", textTransform: "uppercase", letterSpacing: "0.5px" }}>
            {activeBot.icon}
            <span>{activeBot.label}</span>
          </div>
          <div style={{ display: "flex", gap: "4px", alignItems: "center" }}>
            <button
              type="button"
              onClick={handleRefresh}
              style={{ background: "transparent", border: "none", color: "var(--fg-muted, #888)", cursor: "pointer", padding: "4px", borderRadius: "4px" }}
              title="Reload"
            >
              <RefreshCw size={14} />
            </button>
            <button
              type="button"
              onClick={() => void openUrl(activeBot.url)}
              style={{ background: "transparent", border: "none", color: "var(--fg-muted, #888)", cursor: "pointer", padding: "4px", borderRadius: "4px" }}
              title="Open in browser"
            >
              <ExternalLink size={14} />
            </button>
            <button
              type="button"
              onClick={() => void handleChange()}
              style={{
                background: "transparent",
                border: "none",
                color: "var(--fg-muted, #888)",
                cursor: "pointer",
                fontSize: "11px",
                padding: "4px 8px",
              }}
            >
              Change
            </button>
          </div>
        </div>

        {/* Webview container — the native webview is positioned over this element */}
        <div
          ref={containerRef}
          style={{ flex: 1, width: "100%", position: "relative", background: "#1a1a1a" }}
        >
          {isWebviewLoading && !webviewError && (
            <div className="ai-webview-status">Loading {activeBot.label}…</div>
          )}
          {webviewError && (
            <div className="ai-webview-error">
              <p style={{ marginBottom: "12px" }}>Could not embed {activeBot.label} inline.</p>
              <p className="ai-webview-error-detail">{webviewError}</p>
              <button
                type="button"
                onClick={() => void openUrl(activeBot.url)}
                style={{
                  padding: "8px 16px",
                  background: activeBot.color,
                  color: "#000",
                  border: "none",
                  borderRadius: "6px",
                  fontSize: "13px",
                  fontWeight: 600,
                  cursor: "pointer",
                }}
              >
                Open in Browser Instead
              </button>
            </div>
          )}
        </div>
      </div>
    );
  }

  // ── Selection screen ──
  return (
    <div className="mcp-view" style={{ height: "100%", display: "flex", flexDirection: "column", background: "var(--bg-base, #111)", alignItems: "center", justifyContent: "center", padding: "20px" }}>
      <div style={{
        width: "100%",
        maxWidth: "400px",
        border: "1px solid #E8C15A",
        borderRadius: "12px",
        padding: "32px 24px",
        display: "flex",
        flexDirection: "column",
        gap: "24px",
        background: "#18181a"
      }}>
        <div style={{ textAlign: "center", display: "flex", flexDirection: "column", gap: "8px" }}>
          <h2 style={{ fontSize: "20px", fontWeight: 500, color: "#fff", margin: 0 }}>
            Choose an AI chatbot to use in the Hopper sidebar
          </h2>
          <p style={{ fontSize: "13px", color: "#a1a1aa", margin: 0 }}>
            Switch anytime. The chatbot opens right here in the sidebar.
          </p>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
          {CHATBOTS.map((bot) => {
            const isSelected = selectedChatbot === bot.id;
            return (
              <button
                key={bot.id}
                type="button"
                onClick={() => setSelectedChatbot(bot.id)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: "12px",
                  padding: "12px 16px",
                  background: isSelected ? "#27272a" : "#1f1f22",
                  border: `1px solid ${isSelected ? "#E8C15A" : "#3f3f46"}`,
                  borderRadius: "8px",
                  color: "#fff",
                  cursor: "pointer",
                  width: "100%",
                  textAlign: "left",
                  fontSize: "14px",
                  fontWeight: 500,
                  transition: "all 0.2s ease"
                }}
              >
                {bot.icon}
                {bot.label}
              </button>
            );
          })}
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: "12px", marginTop: "8px" }}>
          <button
            type="button"
            onClick={handleContinue}
            disabled={!selectedChatbot}
            style={{
              width: "100%",
              padding: "12px",
              background: "#E8C15A",
              color: "#000",
              border: "none",
              borderRadius: "8px",
              fontSize: "14px",
              fontWeight: 600,
              cursor: selectedChatbot ? "pointer" : "not-allowed",
              opacity: selectedChatbot ? 1 : 0.5,
            }}
          >
            Continue
          </button>
        </div>
      </div>
    </div>
  );
}
