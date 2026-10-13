import { lazy, Suspense, useCallback, useState } from "react";
import { ClaudeCommandDialog, type ClaudeCommandDialogProps } from "@/features/claude/components/ClaudeCommandDialog";
import { parseClaudeCommand, type ClaudeDialog } from "@/features/claude/utils/commands";
import { notifySlashCommandsChanged } from "@services/events";

const ClaudePluginsDialog = lazy(() => import("@/features/claude/components/ClaudePluginsDialog").then((module) => ({ default: module.ClaudePluginsDialog })));

type Options = Omit<ClaudeCommandDialogProps, "kind" | "onClose" | "onInsert"> & {
  provider: string;
  workspaceId: string | null;
  draftKey: string | null;
  onPlanModeToggle?: (enabled: boolean) => void;
  setDraft: (text: string) => void;
  focusComposer: () => void;
};

export function useClaudeComposerCommands(options: Options) {
  const { provider, workspaceId, draftKey, setDraft, focusComposer, onPlanModeToggle, isPlanMode, models, onSelectModel } = options;
  const scope = JSON.stringify([provider, workspaceId, draftKey]);
  const [dialog, setDialog] = useState<{ kind: ClaudeDialog; scope: string } | null>(null);
  const close = useCallback(() => { setDialog(null); focusComposer(); }, [focusComposer]);
  const insert = (text: string) => { setDraft(text); close(); };
  const run = useCallback((text: string): { nextDraft: string } | null => {
    if (provider !== "claude") return null;
    const command = parseClaudeCommand(text);
    if (!command || command.action === "provider" || command.action === "new") return null;
    if (command.action === "plan") {
      // Keep a planning prompt in the editor until the user sends it, with the
      // selected plan mode visible. Never send a control command as prose.
      const value = command.args.toLowerCase();
      onPlanModeToggle?.(value === "off" ? false : value === "on" || Boolean(value) ? true : !isPlanMode);
      return { nextDraft: value === "on" || value === "off" ? "" : command.args };
    }
    if (command.action === "model" && command.args) {
      const query = command.args.toLowerCase();
      const exact = models.find((model) => [model.id, model.model, model.displayName].some((value) => value.toLowerCase() === query));
      if (exact) { onSelectModel(exact.id); return { nextDraft: "" }; }
    }
    setDialog({ kind: command.action, scope });
    return { nextDraft: "" };
  }, [isPlanMode, onPlanModeToggle, models, onSelectModel, provider, scope]);

  const visible = provider === "claude" && dialog?.scope === scope ? dialog.kind : null;
  const node = visible === "plugins" ? (
    <Suspense fallback={<div role="status" className="composer-command-loading">Opening Claude plugins…</div>}>
      <ClaudePluginsDialog key={scope} workspaceId={workspaceId} onClose={close} onChanged={() => notifySlashCommandsChanged("claude")} />
    </Suspense>
  ) : visible ? (
    <ClaudeCommandDialog {...options} key={`${scope}:${visible}`} kind={visible} onClose={close} onInsert={insert} />
  ) : null;
  return { run, node };
}
