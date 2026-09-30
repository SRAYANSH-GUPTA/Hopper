import { useCallback, useRef, useState } from "react";
import { getProviderSetupStatus, type SetupProviderId } from "@services/tauri";

const SETUP_PROVIDER_IDS = new Set<SetupProviderId>(["codex", "claude", "antigravity"]);

export function useProviderSwitchGuard(onSwitch: (providerId: string) => void) {
  const [requestedProviderId, setRequestedProviderId] = useState<SetupProviderId | null>(null);
  const [checkingProviderId, setCheckingProviderId] = useState<SetupProviderId | null>(null);
  const requestGeneration = useRef(0);

  const requestSwitch = useCallback(async (providerId: string): Promise<boolean> => {
    if (!SETUP_PROVIDER_IDS.has(providerId as SetupProviderId)) {
      onSwitch(providerId);
      return true;
    }

    const setupProviderId = providerId as SetupProviderId;
    const generation = ++requestGeneration.current;
    setCheckingProviderId(setupProviderId);
    try {
      const status = await getProviderSetupStatus();
      if (generation !== requestGeneration.current) return false;
      const provider = status.providers.find((candidate) => candidate.id === setupProviderId);
      if (!status.supported || !provider || !provider.installed || provider.authenticated === false) {
        setRequestedProviderId(setupProviderId);
        return false;
      }
      onSwitch(providerId);
      return true;
    } catch {
      if (generation === requestGeneration.current) setRequestedProviderId(setupProviderId);
      return false;
    } finally {
      if (generation === requestGeneration.current) setCheckingProviderId(null);
    }
  }, [onSwitch]);

  const finishSetup = useCallback((providerId: SetupProviderId) => {
    setRequestedProviderId(null);
    onSwitch(providerId);
  }, [onSwitch]);

  return {
    requestedProviderId,
    checkingProviderId,
    requestSwitch,
    closeSetup: () => setRequestedProviderId(null),
    finishSetup,
  };
}
