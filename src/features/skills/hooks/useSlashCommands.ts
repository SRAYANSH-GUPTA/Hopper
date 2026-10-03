import { useCallback, useEffect, useRef, useState } from "react";
import { getSlashCommandsList, type SlashCommandOption } from "@services/tauri";

type UseSlashCommandsOptions = {
  workspaceId: string | null;
  provider: string;
};

/**
 * Lists the skills and custom commands installed for `provider`. Refreshes when
 * the window regains focus so newly installed skills appear without a restart.
 */
export function useSlashCommands({ workspaceId, provider }: UseSlashCommandsOptions) {
  const [commands, setCommands] = useState<SlashCommandOption[]>([]);
  const requestRef = useRef(0);

  const refresh = useCallback(async () => {
    const request = ++requestRef.current;
    try {
      const next = await getSlashCommandsList(workspaceId, provider);
      if (request === requestRef.current) {
        setCommands(Array.isArray(next) ? next : []);
      }
    } catch {
      if (request === requestRef.current) setCommands([]);
    }
  }, [provider, workspaceId]);

  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      requestRef.current += 1;
    };
  }, [refresh]);

  return commands;
}
