import { useCallback, useEffect, useRef, useState } from "react";
import {
  getProviderSetupStatus, runProviderSetupAction, saveProviderSetup,
  type ProviderSetupPreferences, type ProviderSetupStatus, type SetupProviderId,
} from "@services/tauri";

export function useProviderSetup(enabled: boolean, targetKey: string) {
  const [status, setStatus] = useState<ProviderSetupStatus | null>(null);
  const [preferences, setPreferences] = useState<ProviderSetupPreferences | null>(null);
  const [verified, setVerified] = useState<Partial<Record<SetupProviderId, boolean>>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [closed, setClosed] = useState(false);
  const generation = useRef(0);
  const operation = useRef(false);

  const refresh = useCallback(async (reset = false) => {
    const current = generation.current;
    if (operation.current) return;
    operation.current = true;
    setBusy("Checking installed providers…");
    setError(null);
    try {
      const result = await getProviderSetupStatus();
      if (current !== generation.current) return;
      setStatus(result);
      setPreferences((previous) => reset || !previous ? result.preferences : previous);
    } catch (e) {
      if (current === generation.current) setError(String(e));
    } finally {
      if (current === generation.current) { operation.current = false; setBusy(null); }
    }
  }, []);

  useEffect(() => {
    generation.current += 1;
    operation.current = false;
    setStatus(null);
    setPreferences(null);
    setVerified({});
    setClosed(false);
    setMessage(null);
    setError(null);
    if (enabled) void refresh(true);
    return () => { generation.current += 1; };
  }, [enabled, targetKey, refresh]);

  const act = async (provider: SetupProviderId, action: "install" | "login" | "verify") => {
    if (operation.current) return;
    operation.current = true;
    const current = generation.current;
    setBusy(action === "install" ? "Installing from the official provider. This may take a few minutes…" : action === "login" ? "Opening sign-in…" : "Testing the connection…");
    setError(null);
    setMessage(null);
    setVerified((prev) => ({ ...prev, [provider]: false }));
    try {
      const result = await runProviderSetupAction(provider, action);
      if (current !== generation.current) return;
      setMessage(result.message);
      if (result.verified) setVerified((prev) => ({ ...prev, [provider]: true }));
      const next = await getProviderSetupStatus();
      if (current === generation.current) setStatus(next);
    } catch (e) {
      if (current === generation.current) setError(String(e));
    } finally {
      if (current === generation.current) { operation.current = false; setBusy(null); }
    }
  };

  const finish = async (skip: boolean, onConfigured?: (preferences: ProviderSetupPreferences) => Promise<void>) => {
    if (!preferences || operation.current) return;
    operation.current = true;
    const current = generation.current;
    setBusy("Saving setup…");
    setError(null);
    try {
      const next = { ...preferences, completed: true };
      if (!skip) await onConfigured?.(next);
      // "Set up later" dismisses onboarding, but does not claim a verified connection.
      await saveProviderSetup(next);
      if (current === generation.current) { setPreferences(next); setClosed(true); setMessage("Setup preferences saved."); }
    } catch (e) {
      if (current === generation.current) setError(String(e));
    } finally {
      if (current === generation.current) { operation.current = false; setBusy(null); }
    }
  };

  const selected = preferences ? [preferences.claudeEnabled && "claude", preferences.antigravityEnabled && "antigravity"].filter(Boolean) as SetupProviderId[] : [];
  return { status, preferences, setPreferences, verified, busy, error, message, closed, dismiss: () => setClosed(true), refresh, act, finish,
    canFinish: selected.length > 0 && selected.every((id) => verified[id]) };
}
