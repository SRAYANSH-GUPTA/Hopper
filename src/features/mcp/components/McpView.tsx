import { useState, useMemo, useEffect, useCallback } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Github, ExternalLink, Copy, Check, Zap, X, Loader2, PlugZap, Plug } from "lucide-react";
import { MCP_SERVERS, MCP_CATEGORIES } from "../mcpData";
import type { McpCategory, McpServer, McpEnvKey } from "../mcpData";
import { mcpListServers, mcpAddServer, mcpRemoveServer } from "../../../services/tauri";
import type { McpServerConfig } from "../../../services/tauri";

function copyToClipboard(text: string, setCopied: (id: string | null) => void, id: string) {
  void navigator.clipboard.writeText(text).then(() => {
    setCopied(id);
    setTimeout(() => setCopied(null), 1800);
  });
}

// ─── Connect Dialog ───────────────────────────────────────────────────────────

function ConnectDialog({
  server,
  onClose,
  onConnected,
}: {
  server: McpServer;
  onClose: () => void;
  onConnected: () => void;
}) {
  const cfg = server.installConfig!;
  const [values, setValues] = useState<Record<string, string>>(() => {
    const init: Record<string, string> = {};
    for (const k of cfg.envKeys) init[k.key] = "";
    return init;
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleConnect() {
    setSaving(true);
    setError(null);
    try {
      const env: Record<string, string> = {};
      for (const k of cfg.envKeys) {
        if (values[k.key]) env[k.key] = values[k.key];
      }
      const config: McpServerConfig = {
        command: cfg.command,
        args: cfg.args,
        env: Object.keys(env).length > 0 ? env : undefined,
      };
      await mcpAddServer(server.id, config);
      onConnected();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  const allFilled = cfg.envKeys.every((k) => !k.secret || values[k.key]?.trim());

  return (
    <div className="mcp-dialog-backdrop" onClick={onClose}>
      <div className="mcp-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="mcp-dialog-header">
          <span className="mcp-dialog-icon">{server.icon}</span>
          <div>
            <div className="mcp-dialog-title">Connect {server.name}</div>
            <div className="mcp-dialog-sub">
              Writes config to ~/.claude/settings.json
            </div>
          </div>
          <button type="button" className="mcp-dialog-close" onClick={onClose}>
            <X size={14} />
          </button>
        </div>

        {cfg.envKeys.length === 0 ? (
          <p className="mcp-dialog-noenv">
            No credentials required — click Connect to enable.
          </p>
        ) : (
          <div className="mcp-dialog-fields">
            {cfg.envKeys.map((k: McpEnvKey) => (
              <div key={k.key} className="mcp-dialog-field">
                <label className="mcp-dialog-label">
                  {k.label}
                  {k.secret && <span className="mcp-dialog-secret">secret</span>}
                </label>
                <input
                  className="mcp-dialog-input"
                  type={k.secret ? "password" : "text"}
                  placeholder={k.placeholder ?? ""}
                  value={values[k.key] ?? ""}
                  onChange={(e) =>
                    setValues((v) => ({ ...v, [k.key]: e.target.value }))
                  }
                  autoComplete="off"
                  spellCheck={false}
                />
              </div>
            ))}
          </div>
        )}

        {error && <div className="mcp-dialog-error">{error}</div>}

        <div className="mcp-dialog-actions">
          <button type="button" className="mcp-dialog-cancel" onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            className="mcp-dialog-connect"
            onClick={() => void handleConnect()}
            disabled={saving || !allFilled}
          >
            {saving ? <Loader2 size={12} className="mcp-spin" /> : <PlugZap size={12} />}
            {saving ? "Connecting…" : "Connect"}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── McpCard ──────────────────────────────────────────────────────────────────

function McpCard({
  server,
  copiedId,
  onCopy,
  connected,
  onConnect,
  onDisconnect,
}: {
  server: McpServer;
  copiedId: string | null;
  onCopy: (id: string) => void;
  connected: boolean;
  onConnect: (server: McpServer) => void;
  onDisconnect: (id: string) => void;
}) {
  const isCopied = copiedId === server.id;
  const canConnect = Boolean(server.installConfig);

  return (
    <div className={`mcp-card${connected ? " mcp-card-connected" : ""}`}>
      <div className="mcp-card-top">
        <span className="mcp-card-icon" aria-hidden>{server.icon}</span>
        <div className="mcp-card-copy">
          <div className="mcp-card-name-row">
            <span className="mcp-card-name">{server.name}</span>
            {server.official && <span className="mcp-card-badge">official</span>}
            {connected && (
              <span className="mcp-card-badge mcp-card-badge-live">
                <span className="mcp-live-dot" />
                live
              </span>
            )}
          </div>
          <span className="mcp-card-cat">{server.category}</span>
        </div>
        {server.stars && (
          <span className="mcp-card-stars">⭐ {server.stars}</span>
        )}
      </div>

      <p className="mcp-card-desc">{server.description}</p>

      {server.install && (
        <div className="mcp-card-install">
          <code className="mcp-card-cmd">{server.install}</code>
          <button
            type="button"
            className="mcp-card-copy-btn"
            onClick={() => onCopy(server.id)}
            aria-label="Copy install command"
            title="Copy"
          >
            {isCopied ? <Check size={11} /> : <Copy size={11} />}
          </button>
        </div>
      )}

      <div className="mcp-card-actions">
        <button
          type="button"
          className="mcp-card-btn mcp-card-btn-github"
          onClick={() => void openUrl(server.repo)}
        >
          <Github size={12} aria-hidden />
          GitHub
        </button>
        <button
          type="button"
          className="mcp-card-btn mcp-card-btn-open"
          onClick={() => void openUrl(server.repo)}
          aria-label="Open repo"
        >
          <ExternalLink size={11} aria-hidden />
        </button>

        {canConnect && (
          connected ? (
            <button
              type="button"
              className="mcp-card-btn mcp-card-btn-disconnect"
              onClick={() => onDisconnect(server.id)}
            >
              <Plug size={11} aria-hidden />
              Disconnect
            </button>
          ) : (
            <button
              type="button"
              className="mcp-card-btn mcp-card-btn-connect"
              onClick={() => onConnect(server)}
            >
              <PlugZap size={11} aria-hidden />
              Connect
            </button>
          )
        )}
      </div>
    </div>
  );
}

// ─── McpView ─────────────────────────────────────────────────────────────────

export function McpView() {
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<McpCategory>("All");
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [connectedIds, setConnectedIds] = useState<Set<string>>(new Set());
  const [dialogServer, setDialogServer] = useState<McpServer | null>(null);

  // Load currently connected servers from ~/.claude/settings.json
  const refreshConnected = useCallback(async () => {
    try {
      const servers = await mcpListServers();
      setConnectedIds(new Set(Object.keys(servers)));
    } catch {
      // Ignore — settings file may not exist yet
    }
  }, []);

  useEffect(() => {
    void refreshConnected();
  }, [refreshConnected]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return MCP_SERVERS.filter((s) => {
      const matchCat = category === "All" || s.category === category;
      const matchQ =
        !q ||
        s.name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q);
      return matchCat && matchQ;
    });
  }, [query, category]);

  function handleCopy(id: string) {
    const server = MCP_SERVERS.find((s) => s.id === id);
    if (server?.install) {
      copyToClipboard(server.install, setCopiedId, id);
    }
  }

  function handleConnect(server: McpServer) {
    // If no env keys required, connect immediately without dialog
    if (server.installConfig && server.installConfig.envKeys.length === 0) {
      void (async () => {
        try {
          await mcpAddServer(server.id, {
            command: server.installConfig!.command,
            args: server.installConfig!.args,
          });
          await refreshConnected();
        } catch (err) {
          console.error("MCP connect failed:", err);
        }
      })();
    } else {
      setDialogServer(server);
    }
  }

  async function handleDisconnect(id: string) {
    try {
      await mcpRemoveServer(id);
      await refreshConnected();
    } catch (err) {
      console.error("MCP disconnect failed:", err);
    }
  }

  return (
    <div className="mcp-view">
      {/* Header */}
      <div className="mcp-header">
        <div className="mcp-header-title">
          <Zap size={13} className="mcp-header-icon" aria-hidden />
          MCP SERVERS
        </div>
        <a
          className="mcp-header-registry"
          onClick={() => void openUrl("https://registry.modelcontextprotocol.io")}
          role="button"
          title="Open official MCP registry"
        >
          Registry ↗
        </a>
      </div>

      {/* Connected count pill */}
      {connectedIds.size > 0 && (
        <div className="mcp-connected-bar">
          <span className="mcp-live-dot" />
          {connectedIds.size} server{connectedIds.size !== 1 ? "s" : ""} connected — Claude can use them now
        </div>
      )}

      {/* Search */}
      <div className="mcp-search-wrap">
        <input
          className="mcp-search"
          type="search"
          placeholder="Search servers…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Search MCP servers"
        />
      </div>

      {/* Category chips */}
      <div className="mcp-cats">
        {MCP_CATEGORIES.map((cat) => (
          <button
            key={cat}
            type="button"
            className={`mcp-cat-btn${category === cat ? " is-active" : ""}`}
            onClick={() => setCategory(cat)}
          >
            {cat}
          </button>
        ))}
      </div>

      {/* Count */}
      <div className="mcp-count">
        {filtered.length} server{filtered.length !== 1 ? "s" : ""}
      </div>

      {/* List */}
      <div className="mcp-list">
        {filtered.length === 0 ? (
          <div className="mcp-empty">No servers match "{query}"</div>
        ) : (
          filtered.map((server) => (
            <McpCard
              key={server.id}
              server={server}
              copiedId={copiedId}
              onCopy={handleCopy}
              connected={connectedIds.has(server.id)}
              onConnect={handleConnect}
              onDisconnect={(id) => void handleDisconnect(id)}
            />
          ))
        )}
      </div>

      {/* Connect dialog */}
      {dialogServer && (
        <ConnectDialog
          server={dialogServer}
          onClose={() => setDialogServer(null)}
          onConnected={async () => {
            setDialogServer(null);
            await refreshConnected();
          }}
        />
      )}
    </div>
  );
}
