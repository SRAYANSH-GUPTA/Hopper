import { useCallback, useEffect, useRef, useState } from "react";
import { getSlashCommandsList, type SlashCommandOption } from "@services/tauri";
import { subscribeSlashCommandsChanged } from "@services/events";

type UseSlashCommandsOptions = {
  workspaceId: string | null;
  provider: string;
};

/**
 * Lists the skills and custom commands installed for `provider`. Refreshes when
 * the window regains focus so newly installed skills appear without a restart.
 */
export function useSlashCommands({ workspaceId, provider }: UseSlashCommandsOptions) {
  const scope = JSON.stringify([workspaceId, provider]);
  const [result, setResult] = useState<{ scope: string; commands: SlashCommandOption[] } | null>(null);
  const requestRef = useRef(0);

  const refresh = useCallback(async () => {
    const request = ++requestRef.current;
    try {
      const next = await getSlashCommandsList(workspaceId, provider);
      if (request === requestRef.current) {
        setResult({ scope, commands: Array.isArray(next) ? next : [] });
      }
    } catch {
      if (request === requestRef.current) setResult({ scope, commands: [] });
    }
  }, [provider, workspaceId, scope]);

  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    const unsubscribe = subscribeSlashCommandsChanged((changedProvider) => {
      if (changedProvider === provider) void refresh();
    });
    return () => {
      window.removeEventListener("focus", onFocus);
      unsubscribe();
      requestRef.current += 1;
    };
  }, [provider, refresh]);

  return result?.scope === scope ? result.commands : [];
}
