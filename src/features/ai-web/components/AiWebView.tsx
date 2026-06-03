import { useState } from "react";
import { Bot, MessageSquare, ExternalLink } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";

export function AiWebView() {
  const [activeTab, setActiveTab] = useState<"chatgpt" | "claude">("chatgpt");

  return (
    <div className="mcp-view" style={{ height: "100%", display: "flex", flexDirection: "column" }}>
      <div className="mcp-header" style={{ padding: "12px 16px", borderBottom: "1px solid var(--border)", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <div className="mcp-header-title" style={{ display: "flex", alignItems: "center", gap: "8px" }}>
          <Bot size={16} className="mcp-header-icon" aria-hidden />
          AI ASSISTANTS
        </div>
        <button
          type="button"
          onClick={() => void openUrl(activeTab === "chatgpt" ? "https://chatgpt.com" : "https://claude.ai")}
          style={{
            display: "flex",
            alignItems: "center",
            gap: "6px",
            background: "transparent",
            border: "none",
            color: "var(--fg-muted)",
            cursor: "pointer",
            fontSize: "12px",
          }}
          title="Open in Browser"
        >
          <ExternalLink size={14} />
          Open External
        </button>
      </div>

      <div style={{ display: "flex", padding: "8px", gap: "8px", borderBottom: "1px solid var(--border)" }}>
        <button
          type="button"
          onClick={() => setActiveTab("chatgpt")}
          style={{
            flex: 1,
            padding: "6px 12px",
            background: activeTab === "chatgpt" ? "var(--bg-active, #2a2a2a)" : "transparent",
            border: "1px solid var(--border)",
            borderRadius: "4px",
            color: activeTab === "chatgpt" ? "var(--fg)" : "var(--fg-muted)",
            cursor: "pointer",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: "6px"
          }}
        >
          <MessageSquare size={14} />
          ChatGPT
        </button>
        <button
          type="button"
          onClick={() => setActiveTab("claude")}
          style={{
            flex: 1,
            padding: "6px 12px",
            background: activeTab === "claude" ? "var(--bg-active, #2a2a2a)" : "transparent",
            border: "1px solid var(--border)",
            borderRadius: "4px",
            color: activeTab === "claude" ? "var(--fg)" : "var(--fg-muted)",
            cursor: "pointer",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: "6px"
          }}
        >
          <Bot size={14} />
          Claude
        </button>
      </div>

      <div style={{ flex: 1, width: "100%", background: "#fff" }}>
        {activeTab === "chatgpt" ? (
          <iframe
            src="https://chatgpt.com"
            title="ChatGPT"
            style={{ width: "100%", height: "100%", border: "none" }}
            sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
          />
        ) : (
          <iframe
            src="https://claude.ai"
            title="Claude"
            style={{ width: "100%", height: "100%", border: "none" }}
            sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
          />
        )}
      </div>
    </div>
  );
}
