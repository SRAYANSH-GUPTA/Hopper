import { useCallback, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import X from "lucide-react/dist/esm/icons/x";
import RefreshCw from "lucide-react/dist/esm/icons/refresh-cw";
import { ModalShell } from "@/features/design-system/components/modal/ModalShell";
import { useSetupDialogFocus } from "@app/hooks/useSetupDialogFocus";
import { providerUsageOutput, type SlashCommandOption } from "@services/tauri";
import type { ThreadTokenUsage } from "@/types";
import { CLAUDE_COMMANDS, type ClaudeDialog } from "../utils/commands";
import "@/styles/claude-command-dialog.css";

export type ClaudeCommandDialogProps = {
  kind: Exclude<ClaudeDialog, "plugins">;
  workspacePath?: string | null;
  contextUsage: ThreadTokenUsage | null;
  models: { id: string; displayName: string; model: string }[];
  selectedModelId: string | null;
  accessMode: "read-only" | "current" | "full-access";
  isPlanMode: boolean;
  commands: SlashCommandOption[];
  onSelectModel: (id: string) => void;
  onSelectAccessMode: (mode: "read-only" | "current" | "full-access") => void;
  onInsert: (text: string) => void;
  onClose: () => void;
};

const titles = {
  usage: "Claude usage", help: "Claude commands", skills: "Claude skills & commands",
  model: "Claude model", permissions: "Claude permissions", status: "Claude session",
};
const permissionChoices = [
  { id: "read-only", name: "Read only", description: "Inspect and plan without changing project files." },
  { id: "current", name: "Ask for permission", description: "Ask before tools that require approval." },
  { id: "full-access", name: "Full access", description: "Allow tools without asking. Use only with projects you trust." },
] as const;

export function ClaudeCommandDialog({
  kind, workspacePath, contextUsage, models, selectedModelId, accessMode, isPlanMode,
  commands, onSelectModel, onSelectAccessMode, onInsert, onClose,
}: ClaudeCommandDialogProps) {
  const root = useRef<HTMLDivElement | null>(null);
  const titleId = useId();
  const [query, setQuery] = useState("");
  const [usage, setUsage] = useState<{ loading: boolean; output: string | null; error: string | null }>({ loading: false, output: null, error: null });
  const request = useRef(0);
  useSetupDialogFocus(root, true);
  const refreshUsage = useCallback(async () => {
    const id = ++request.current;
    setUsage({ loading: true, output: null, error: null });
    try {
      const output = await providerUsageOutput("claude", workspacePath ?? null);
      if (request.current === id) setUsage({ loading: false, output, error: null });
    } catch (error) {
      if (request.current === id) setUsage({ loading: false, output: null, error: error instanceof Error ? error.message : String(error) });
    }
  }, [workspacePath]);
  useEffect(() => {
    if (kind === "usage") void refreshUsage();
    return () => { request.current += 1; };
  }, [kind, refreshUsage]);

  const currentModel = models.find((model) => model.id === selectedModelId);
  const filtered = commands.filter((command) => `${command.name} ${command.description ?? ""} ${command.plugin ?? ""}`.toLowerCase().includes(query.toLowerCase()));
  const count = (value: number | undefined) => value === undefined ? "Not reported" : value.toLocaleString();
  const closeAfter = (action: () => void) => { action(); onClose(); };

  return createPortal(
    <ModalShell className="claude-command-modal" cardClassName="claude-command-card" ariaLabelledBy={titleId} onBackdropClick={onClose}>
      <div ref={root} className="claude-command-content" tabIndex={-1} onKeyDown={(event) => {
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onClose(); }
      }}>
        <header className="claude-command-header">
          <h2 id={titleId}>{titles[kind]}</h2>
          <button type="button" className="ghost" aria-label="Close Claude commands" onClick={onClose}><X size={18} aria-hidden /></button>
        </header>
        <div className="claude-command-body">
          {kind === "usage" && <>
            <h3>This conversation</h3>
            <dl className="claude-command-facts">
              <div><dt>Total tokens</dt><dd>{count(contextUsage?.total.totalTokens)}</dd></div>
              <div><dt>Input tokens</dt><dd>{count(contextUsage?.total.inputTokens)}</dd></div>
              <div><dt>Output tokens</dt><dd>{count(contextUsage?.total.outputTokens)}</dd></div>
              <div><dt>Cached input</dt><dd>{count(contextUsage?.total.cachedInputTokens)}</dd></div>
              <div><dt>Last reported context</dt><dd>{count(contextUsage?.last.totalTokens)}</dd></div>
              <div><dt>Context window</dt><dd>{count(contextUsage?.modelContextWindow ?? undefined)}</dd></div>
            </dl>
            <p className="claude-command-note">Token counts are reported by Claude for this conversation. They are separate from your account’s plan limits.</p>
            <div className="claude-command-section-heading"><h3>Claude usage report</h3>
              <button type="button" className="secondary" disabled={usage.loading} onClick={() => void refreshUsage()}><RefreshCw size={14} aria-hidden />Refresh</button>
            </div>
            {usage.loading && <p role="status">Checking Claude usage…</p>}
            {usage.output && <pre className="claude-command-output">{usage.output}</pre>}
            {usage.error && <p role="alert" className="claude-command-error">{usage.error}</p>}
            <p className="claude-command-note">If your CLI or account does not expose limits here, check Usage in Claude Code or your Claude account settings. Dollar costs are not inferred from token counts.</p>
          </>}
          {kind === "status" && <dl className="claude-command-facts">
            <div><dt>Provider</dt><dd>Claude Code</dd></div>
            <div><dt>Model</dt><dd>{currentModel?.displayName ?? selectedModelId ?? "Default"}</dd></div>
            <div><dt>Permissions</dt><dd>{permissionChoices.find((entry) => entry.id === accessMode)?.name}</dd></div>
            <div><dt>Plan mode</dt><dd>{isPlanMode ? "On" : "Off"}</dd></div>
            <div><dt>Installed commands</dt><dd>{commands.length}</dd></div>
          </dl>}
          {kind === "model" && <>
            <p className="claude-command-note">Changes apply to upcoming messages in Hopper.</p>
            <div className="claude-command-list">{models.map((model) => <button key={model.id} type="button" className="claude-command-choice" aria-pressed={model.id === selectedModelId} onClick={() => closeAfter(() => onSelectModel(model.id))}>
              <strong>{model.displayName}</strong><span>{model.model}{model.id === selectedModelId ? " · Selected" : ""}</span>
            </button>)}</div>
            {!models.length && <p role="status">No Claude models are available yet. Reconnect the workspace and try again.</p>}
          </>}
          {kind === "permissions" && <>
            <p className="claude-command-note">Changes apply to upcoming messages. Plan mode takes precedence while it is enabled.</p>
            <div className="claude-command-list">{permissionChoices.map((choice) => <button key={choice.id} type="button" className="claude-command-choice" aria-pressed={accessMode === choice.id} onClick={() => closeAfter(() => onSelectAccessMode(choice.id))}>
              <strong>{choice.name}{accessMode === choice.id ? " · Selected" : ""}</strong><span>{choice.description}</span>
            </button>)}</div>
          </>}
          {kind === "help" && <>
            <p className="claude-command-note">Type / in the composer, choose with ↑ ↓, then Tab or Enter to insert. Press Enter again to run. Installed skills and plugin commands appear in the same menu.</p>
            <div className="claude-command-list">{CLAUDE_COMMANDS.map((command) => <button key={command.name} type="button" className="claude-command-choice" onClick={() => onInsert(`/${command.name} `)}>
              <strong>/{command.name}</strong><span>{command.description}</span>
            </button>)}</div>
            <p className="claude-command-note">Aliases: /plugin → /plugins, /models → /model, /quota and /cost → /usage. Terminal-only Claude commands may not be available in Hopper.</p>
          </>}
          {kind === "skills" && <>
            <label className="claude-command-search">Search installed commands<input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Name, description, or plugin" /></label>
            <div className="claude-command-list">{filtered.map((command) => <button type="button" key={command.name} className="claude-command-choice" onClick={() => onInsert(`/${command.name} `)}>
              <strong>/{command.name}{command.argumentHint ? ` ${command.argumentHint}` : ""}</strong>
              <span>{command.description ?? "Run this installed command"}</span>
              <small>{command.plugin ? `Plugin: ${command.plugin}` : command.scope} · {command.kind}</small>
            </button>)}</div>
            {!filtered.length && <p role="status">{commands.length ? "No commands match your search." : "No installed Claude skills or custom commands were found for this workspace. Add them through /plugins or your .claude/skills directory."}</p>}
          </>}
        </div>
      </div>
    </ModalShell>, document.body,
  );
}
