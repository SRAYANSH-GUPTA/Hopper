import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import X from "lucide-react/dist/esm/icons/x";
import RefreshCw from "lucide-react/dist/esm/icons/refresh-cw";
import { ModalShell } from "@/features/design-system/components/modal/ModalShell";
import type { ClaudePlugin, ClaudePluginScope } from "@services/tauri";
import { useClaudePlugins } from "../hooks/useClaudePlugins";
import { useClaudeDialogFocus } from "../hooks/useClaudeDialogFocus";
import "@/styles/claude-commands.css";

type Props = {
  workspaceId: string | null;
  onClose: () => void;
  onChanged?: (workspaceId: string | null) => void | Promise<void>;
};

function editableScope(plugin: ClaudePlugin): ClaudePluginScope | null {
  return plugin.canModify && ["user", "project", "local"].includes(plugin.scope)
    ? plugin.scope as ClaudePluginScope
    : null;
}

export function ClaudePluginsDialog({ workspaceId, onClose, onChanged }: Props) {
  const titleId = useId();
  const descriptionId = useId();
  const pluginIdInputId = useId();
  const scopeInputId = useId();
  const contentRef = useRef<HTMLDivElement | null>(null);
  const cancelRef = useRef<HTMLButtonElement | null>(null);
  const uninstallTrigger = useRef<HTMLButtonElement | null>(null);
  const [pluginId, setPluginId] = useState("");
  const [scope, setScope] = useState<ClaudePluginScope>("user");
  const [formError, setFormError] = useState<string | null>(null);
  const [confirmation, setConfirmation] = useState<ClaudePlugin | null>(null);
  const { plugins, loading, loadError, actionError, pendingAction, message, refresh, runAction } = useClaudePlugins(workspaceId, onChanged);
  const busy = Boolean(pendingAction);
  const disabled = busy || loading;
  useClaudeDialogFocus(contentRef);

  useEffect(() => {
    setConfirmation(null);
    setScope("user");
    setFormError(null);
  }, [workspaceId]);
  useEffect(() => { if (confirmation) cancelRef.current?.focus(); }, [confirmation]);

  const cancelUninstall = () => {
    setConfirmation(null);
    uninstallTrigger.current?.focus();
  };
  const install = async (event: FormEvent) => {
    event.preventDefault();
    if (disabled) return;
    const submittedId = pluginId.trim();
    if (submittedId.length > 256 || !/^[a-zA-Z0-9][a-zA-Z0-9._-]*@[a-zA-Z0-9][a-zA-Z0-9._-]*$/.test(submittedId)) {
      setFormError("Enter a plugin ID such as name@marketplace from a configured marketplace.");
      return;
    }
    setFormError(null);
    if (await runAction({ action: "install", plugin: submittedId, scope })) {
      setPluginId((current) => current.trim() === submittedId ? "" : current);
    }
  };

  return createPortal(<ModalShell ariaLabelledBy={titleId} ariaDescribedBy={descriptionId} cardClassName="claude-plugins-modal" onBackdropClick={onClose}>
    <div ref={contentRef} tabIndex={-1} className="claude-plugins-content" onKeyDown={(event) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        if (confirmation && !busy) cancelUninstall();
        else onClose();
      }
    }}>
      <header className="claude-plugins-heading">
        <div>
          <h2 id={titleId} className="ds-modal-title">Claude Code plugins</h2>
          <p id={descriptionId} className="ds-modal-subtitle">Manage installed Claude Code plugins. Changes apply to the next turn.</p>
        </div>
        <button type="button" className="ghost claude-plugins-close" aria-label="Close plugins" onClick={onClose}><X size={18} aria-hidden /></button>
      </header>
      <section className="claude-plugins-installed" aria-label="Installed plugins" aria-busy={loading}>
        <div className="claude-plugins-heading">
          <h3>Installed{plugins ? ` (${plugins.length})` : ""}</h3>
          <button type="button" className="secondary" disabled={disabled} onClick={() => void refresh()}><RefreshCw size={14} aria-hidden />Refresh</button>
        </div>
        {loading && <p role="status">Loading plugins…</p>}
        {loadError && <div role="alert" className="ds-modal-error">
          <p>Could not load plugins: {loadError}</p>
          {plugins && <p>The list below may be out of date.</p>}
          <button type="button" className="secondary" disabled={disabled} onClick={() => void refresh()}>Retry loading plugins</button>
        </div>}
        {!loading && !loadError && plugins?.length === 0 && <p className="claude-plugins-muted">No plugins are installed. Install one below using its marketplace ID.</p>}
        <ul className="claude-plugin-list">
          {plugins?.map((plugin) => {
            const actionScope = editableScope(plugin);
            const canAct = Boolean(actionScope && (actionScope === "user" || workspaceId));
            return <li key={`${plugin.id}:${plugin.scope}:${plugin.projectPath ?? ""}`} className="claude-plugin" aria-label={`${plugin.name}, ${plugin.scope} scope`}>
              <div className="claude-plugin-details">
                <strong>{plugin.name}</strong>
                <code>{plugin.id}</code>
                <div className="claude-plugin-meta">
                  <span>{plugin.scope} scope</span>
                  {plugin.marketplace && <span>{plugin.marketplace}</span>}
                  {plugin.version && <span>v{plugin.version}</span>}
                  <span>{typeof plugin.enabled !== "boolean" ? "Status unavailable" : plugin.enabled ? "Enabled" : "Disabled"}</span>
                </div>
                {plugin.projectPath && <span className="claude-plugins-muted">{plugin.projectPath}</span>}
                {!canAct && <span className="claude-plugins-muted">{plugin.canModify ? "Select this plugin’s workspace to manage it." : "Managed outside Hopper; changes are unavailable here."}</span>}
              </div>
              {canAct && <div className="claude-plugin-actions">
                {plugin.enabled !== true && <button type="button" className="secondary" disabled={disabled || Boolean(loadError)} aria-label={`Enable ${plugin.name}`} onClick={() => void runAction({ action: "enable", plugin: plugin.id, scope: actionScope! })}>Enable</button>}
                {plugin.enabled !== false && <button type="button" className="secondary" disabled={disabled || Boolean(loadError)} aria-label={`Disable ${plugin.name}`} onClick={() => void runAction({ action: "disable", plugin: plugin.id, scope: actionScope! })}>Disable</button>}
                <button type="button" className="ghost" disabled={disabled || Boolean(loadError)} aria-label={`Uninstall ${plugin.name}`} onClick={(event) => { uninstallTrigger.current = event.currentTarget; setConfirmation(plugin); }}>Uninstall</button>
              </div>}
            </li>;
          })}
        </ul>
      </section>
      {confirmation && <section className="claude-plugin-confirmation" aria-label={`Confirm uninstall ${confirmation.name}`}>
        <h3>Uninstall {confirmation.name}?</h3>
        <p>This removes the plugin from {confirmation.scope} scope. Its saved data will be kept.</p>
        <div className="claude-plugin-actions">
          <button ref={cancelRef} type="button" className="secondary" disabled={busy} onClick={cancelUninstall}>Cancel uninstall</button>
          <button type="button" className="primary" disabled={disabled || Boolean(loadError)} onClick={async () => {
            const actionScope = editableScope(confirmation);
            if (actionScope && await runAction({ action: "uninstall", plugin: confirmation.id, scope: actionScope })) setConfirmation(null);
          }}>Confirm uninstall</button>
        </div>
      </section>}
      <form className="claude-plugin-install" onSubmit={(event) => void install(event)}>
        <h3>Install a plugin</h3>
        <label className="ds-modal-label" htmlFor={pluginIdInputId}>Plugin ID</label>
        <input id={pluginIdInputId} className="ds-modal-input" value={pluginId} onChange={(event) => { setPluginId(event.target.value); setFormError(null); }} placeholder="name@marketplace" autoComplete="off" spellCheck={false} aria-invalid={Boolean(formError)} />
        <label className="ds-modal-label" htmlFor={scopeInputId}>Install scope</label>
        <select id={scopeInputId} className="ds-modal-input" value={scope} disabled={busy} onChange={(event) => setScope(event.target.value as ClaudePluginScope)}>
          <option value="user">User — all your projects</option>
          <option value="project" disabled={!workspaceId}>Project — shared project settings</option>
          <option value="local" disabled={!workspaceId}>Local — this project, only for you</option>
        </select>
        <p className="claude-plugins-muted">Use a marketplace already configured in Claude Code. Only install plugins you trust.{!workspaceId && " Select a workspace to use Project or Local scope."}</p>
        {formError && <p role="alert" className="ds-modal-error">{formError}</p>}
        <div className="ds-modal-actions"><button type="submit" className="primary" disabled={disabled || !pluginId.trim()}>Install plugin</button></div>
      </form>
      {busy && <p role="status">Applying plugin change…</p>}
      {message && <p role="status">{message} Changes apply to the next turn.</p>}
      {actionError && <p role="alert" className="ds-modal-error">{actionError}</p>}
    </div>
  </ModalShell>, document.body);
}
