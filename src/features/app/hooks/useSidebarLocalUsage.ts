import { useCallback, useEffect, useRef, useState } from "react";
import type { LocalUsageSnapshot } from "../../../types";
import { localUsageSnapshot } from "../../../services/tauri";

type SidebarLocalUsageState = {
  snapshot: LocalUsageSnapshot | null;
  isLoading: boolean;
  error: string | null;
};

const emptyState: SidebarLocalUsageState = {
  snapshot: null,
  isLoading: false,
  error: null,
};

/** Longer refresh interval for sidebar — less aggressive than Home's 5m. */
const REFRESH_INTERVAL_MS = 10 * 60 * 1000;

/**
 * Lightweight local-usage hook for the sidebar bottom rail.
 * Always active (not gated by Home visibility) but uses a longer
 * polling interval to keep backend calls low.
 */
export function useSidebarLocalUsage() {
  const [state, setState] = useState<SidebarLocalUsageState>(emptyState);
  const requestIdRef = useRef(0);

  const refresh = useCallback(() => {
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    setState((prev) => ({ ...prev, isLoading: true, error: null }));
    return localUsageSnapshot(7)
      .then((snapshot) => {
        if (requestIdRef.current !== requestId) {
          return;
        }
        setState({ snapshot, isLoading: false, error: null });
      })
      .catch((err) => {
        if (requestIdRef.current !== requestId) {
          return;
        }
        const message = err instanceof Error ? err.message : String(err);
        setState((prev) => ({ ...prev, isLoading: false, error: message }));
      });
  }, []);

  useEffect(() => {
    refresh()?.catch(() => {});
    const interval = window.setInterval(() => {
      refresh()?.catch(() => {});
    }, REFRESH_INTERVAL_MS);
    return () => {
      window.clearInterval(interval);
    };
  }, [refresh]);

  return { ...state, refresh };
}
