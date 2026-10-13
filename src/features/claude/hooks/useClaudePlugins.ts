import { useCallback, useEffect, useRef, useState } from "react";
import {
  claudePluginAction,
  claudePluginsList,
  type ClaudePlugin,
  type ClaudePluginActionInput,
} from "@services/tauri";

type PluginState = {
  workspaceId: string | null;
  plugins: ClaudePlugin[] | null;
  loading: boolean;
  loadError: string | null;
  actionError: string | null;
  pendingAction: string | null;
  message: string | null;
};

function initialState(workspaceId: string | null): PluginState {
  return { workspaceId, plugins: null, loading: true, loadError: null, actionError: null, pendingAction: null, message: null };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function useClaudePlugins(
  workspaceId: string | null,
  onChanged?: (workspaceId: string | null) => void | Promise<void>,
) {
  const [state, setState] = useState(() => initialState(workspaceId));
  const generation = useRef(0);
  const request = useRef(0);
  const actionPending = useRef(false);

  const refresh = useCallback(async () => {
    const currentGeneration = generation.current;
    const currentRequest = ++request.current;
    setState((prev) => ({ ...prev, loading: true, loadError: null }));
    const isCurrent = () => currentGeneration === generation.current && currentRequest === request.current;
    try {
      const result = await claudePluginsList(workspaceId);
      if (isCurrent()) setState((prev) => ({ ...prev, plugins: result.plugins, loading: false }));
    } catch (error) {
      if (isCurrent()) setState((prev) => ({ ...prev, loading: false, loadError: errorMessage(error) }));
    }
  }, [workspaceId]);

  useEffect(() => {
    generation.current += 1;
    actionPending.current = false;
    setState(initialState(workspaceId));
    void refresh();
    return () => { generation.current += 1; request.current += 1; };
  }, [refresh, workspaceId]);

  const runAction = useCallback(async (
    input: Omit<ClaudePluginActionInput, "workspaceId">,
  ): Promise<boolean> => {
    if (actionPending.current) return false;
    actionPending.current = true;
    const currentGeneration = generation.current;
    setState((prev) => ({ ...prev, pendingAction: input.action, actionError: null, message: null }));
    try {
      const result = await claudePluginAction({ ...input, workspaceId });
      // Command discovery should refresh even if this dialog was closed while
      // the requested CLI operation was finishing.
      let refreshError: string | null = null;
      try {
        await onChanged?.(workspaceId);
      } catch (error) {
        refreshError = `Plugin updated, but commands could not refresh: ${errorMessage(error)}`;
      }
      if (currentGeneration !== generation.current) return false;
      setState((prev) => ({ ...prev, message: result.message || "Plugin updated.", actionError: refreshError }));
      await refresh();
      return currentGeneration === generation.current;
    } catch (error) {
      if (currentGeneration === generation.current) {
        setState((prev) => ({ ...prev, actionError: errorMessage(error) }));
      }
      return false;
    } finally {
      if (currentGeneration === generation.current) {
        actionPending.current = false;
        setState((prev) => ({ ...prev, pendingAction: null }));
      }
    }
  }, [onChanged, refresh, workspaceId]);

  return { ...(state.workspaceId === workspaceId ? state : initialState(workspaceId)), refresh, runAction };
}
