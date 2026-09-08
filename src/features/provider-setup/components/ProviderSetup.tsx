import { useRef } from "react";
import { CheckCircle2, Download, ExternalLink, RefreshCw } from "lucide-react";
import { ModalShell } from "@/features/design-system/components/modal/ModalShell";
import { useProviderSetup } from "@app/hooks/useProviderSetup";
import { useSetupDialogFocus } from "@app/hooks/useSetupDialogFocus";
import type { ProviderSetupPreferences } from "@services/tauri";
import "@/styles/provider-setup.css";

type Props = {
  enabled?: boolean;
  onboarding?: boolean;
  targetKey: string;
  remote?: boolean;
  onConfigured?: (preferences: ProviderSetupPreferences) => Promise<void>;
};

export function ProviderSetup({ enabled = true, onboarding = false, targetKey, remote = false, onConfigured }: Props) {
  const setup = useProviderSetup(enabled, targetKey);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const visible = enabled && (!onboarding || (!setup.closed && ((setup.status?.supported && !setup.status.preferences.completed) || (!setup.status && setup.error))));
  useSetupDialogFocus(contentRef, Boolean(visible && onboarding));
  if (!visible) return null;
  const { preferences, status, busy, error, message } = setup;
  const content = (
    <div className="provider-setup" ref={contentRef} tabIndex={-1} onKeyDown={(event) => {
      if (onboarding && event.key === "Escape" && !setup.busy) {
        event.stopPropagation();
        setup.dismiss();
      }
    }}>
      <div>
        <h2 className="ds-modal-title">Connect your coding agents</h2>
        <p className="ds-modal-subtitle">Choose the providers you want to use. Hopper can install them and help you sign in with your own account.</p>
        {remote && <p className="provider-setup-note">These actions run on your remote host. Complete sign-in in a terminal on that host.</p>}
      </div>
      {!status && !busy && <button className="secondary" onClick={() => void setup.refresh()}>Check providers</button>}
      {status && !status.supported && <p>Install providers on a desktop or connect Hopper to a remote host.</p>}
      {status?.supported && status.providers.map((provider) => {
        const key = provider.id === "claude" ? "claudeEnabled" : "antigravityEnabled";
        const selected = preferences?.[key] ?? false;
        const ready = setup.verified[provider.id];
        return (
          <section className="provider-setup-provider" key={provider.id} aria-label={provider.label}>
            <div className="provider-setup-provider-heading">
              <label className="provider-setup-choice">
                <input type="checkbox" checked={selected} disabled={Boolean(busy)} onChange={(event) => setup.setPreferences((prev) => prev && ({ ...prev, [key]: event.target.checked }))} />
                <strong>{provider.label}</strong>
              </label>
              <span className={`provider-setup-status${ready ? " is-ready" : ""}`}>
                {ready && <CheckCircle2 size={14} aria-hidden />}
                {ready ? "Connection verified" : provider.installed ? provider.authenticated ? "Signed in · test connection" : "Installed" : "Not installed"}
              </span>
            </div>
            {provider.version && <span className="provider-setup-note">{provider.version}</span>}
            {selected && (
              <>
                <div className="provider-setup-actions">
                  {!provider.installed ? (
                    <button className="primary" disabled={Boolean(busy)} onClick={() => void setup.act(provider.id, "install")}><Download size={14} aria-hidden />Install {provider.label}</button>
                  ) : (
                    <>
                      <button className="secondary" disabled={Boolean(busy) || remote} onClick={() => void setup.act(provider.id, "login")}><ExternalLink size={14} aria-hidden />Sign in</button>
                      <button className="secondary" disabled={Boolean(busy)} onClick={() => void setup.act(provider.id, "verify")}>Test connection</button>
                    </>
                  )}
                </div>
                {remote && <p className="provider-setup-note">On the host, run <code>{provider.id === "claude" ? "claude auth login" : "agy"}</code>, complete sign-in, then test here.</p>}
                {provider.id === "antigravity" && (
                  <label className="provider-setup-permissions">
                    <input type="checkbox" checked={preferences?.antigravityAutoApprove ?? false} disabled={Boolean(busy)} onChange={(event) => setup.setPreferences((prev) => prev && ({ ...prev, antigravityAutoApprove: event.target.checked }))} />
                    <span>Allow Antigravity to run tools without asking<p className="provider-setup-note">Allows file changes and terminal commands without approval. Off by default; requests requiring approval may stop in Hopper's headless mode.</p></span>
                  </label>
                )}
              </>
            )}
          </section>
        );
      })}
      <p className="provider-setup-note">Sign-in opens your terminal and may open a browser. Test connection sends a short prompt and may use provider credits. Your existing provider settings and credentials stay with the provider.</p>
      {busy && <p role="status" className="provider-setup-feedback"><RefreshCw size={14} aria-hidden />{busy}</p>}
      {message && !busy && <p role="status" className="provider-setup-feedback">{message}</p>}
      {error && <p role="alert" className="provider-setup-error">{error}</p>}
      <div className="ds-modal-actions">
        <button className="ghost" disabled={Boolean(busy)} onClick={() => void setup.refresh()}>Refresh</button>
        {onboarding ? <>
          <button className="secondary" disabled={Boolean(busy)} onClick={() => preferences ? void setup.finish(true) : setup.dismiss()}>Set up later</button>
          <button className="primary" disabled={Boolean(busy) || !setup.canFinish} onClick={() => void setup.finish(false, onConfigured)}>Start using Hopper</button>
        </> : <button className="primary" disabled={Boolean(busy) || !preferences} onClick={() => void setup.finish(true)}>Save setup preferences</button>}
      </div>
    </div>
  );
  return onboarding ? <ModalShell ariaLabel="Provider setup" cardClassName="provider-setup-modal">{content}</ModalShell> : content;
}
