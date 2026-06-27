import Settings from "lucide-react/dist/esm/icons/settings";

import type { LocalUsageSnapshot } from "../../../types";

type SidebarBottomRailProps = {
  sessionPercent: number | null;
  weeklyPercent: number | null;
  sessionResetLabel: string | null;
  weeklyResetLabel: string | null;
  showWeekly: boolean;
  activeProviderLabel: string;
  localUsageSnapshot?: LocalUsageSnapshot | null;
  isLoadingLocalUsage?: boolean;
  onOpenSettings: () => void;
};

type UsageRowProps = {
  label: string;
  percent: number | null;
  resetLabel: string | null;
};

function UsageRow({ label, percent, resetLabel }: UsageRowProps) {
  return (
    <div className="sidebar-usage-row">
      <div className="sidebar-usage-row-head">
        <span className="sidebar-usage-name">{label}</span>
        <span className="sidebar-usage-value">
          {percent === null ? "--" : `${percent}%`}
        </span>
      </div>
      <div className="sidebar-usage-bar" aria-hidden>
        <span className="sidebar-usage-bar-fill" style={{ width: `${percent ?? 0}%` }} />
      </div>
      {resetLabel && <div className="sidebar-usage-reset">{resetLabel}</div>}
    </div>
  );
}


export function SidebarBottomRail({
  sessionPercent,
  weeklyPercent,
  sessionResetLabel,
  weeklyResetLabel,
  showWeekly,
  activeProviderLabel,
  localUsageSnapshot,
  isLoadingLocalUsage,
  onOpenSettings,
}: SidebarBottomRailProps) {
  return (
    <div className="sidebar-bottom-rail">
      <div className="sidebar-usage-panel">
        {(localUsageSnapshot || isLoadingLocalUsage) && (
          <div className="sidebar-cli-usage-section">
            <div className="sidebar-cli-usage-header">
              <div className="sidebar-usage-kicker">Local Usage</div>
              <div className="sidebar-cli-usage-period">7d</div>
            </div>
            {isLoadingLocalUsage && !localUsageSnapshot ? (
              <div className="sidebar-cli-usage-skeleton">
                <div className="sidebar-cli-usage-skeleton-line" />
                <div className="sidebar-cli-usage-skeleton-line" />
              </div>
            ) : localUsageSnapshot ? (
              <div className="sidebar-cli-usage-grid" style={{ gridTemplateColumns: '1fr' }}>
                <div className="sidebar-cli-usage-stat" style={{ justifyContent: 'center' }}>
                  <span className="sidebar-cli-usage-value" style={{ fontSize: '11px', color: 'var(--text-muted)' }}>
                    Run <code>agy usage</code> for details
                  </span>
                </div>
              </div>
            ) : null}
          </div>
        )}

        <div className="sidebar-usage-header" style={localUsageSnapshot || isLoadingLocalUsage ? { marginTop: '8px' } : undefined}>
          <div className="sidebar-usage-kicker">
            {activeProviderLabel} Usage
          </div>
        </div>
        {sessionPercent === null ? (
          <div className="sidebar-usage-unavailable">
            Not reported by CLI
          </div>
        ) : (
          <div className="sidebar-usage-list">
            <UsageRow
              label="Session (5h)"
              percent={sessionPercent}
              resetLabel={sessionResetLabel}
            />
            {showWeekly && (
              <UsageRow
                label="Weekly"
                percent={weeklyPercent}
                resetLabel={weeklyResetLabel}
              />
            )}
          </div>
        )}
      </div>

      <div className="sidebar-bottom-actions is-compact">
        <div className="sidebar-utility-actions">
          <button
            className="ghost sidebar-labeled-button sidebar-utility-button sidebar-bottom-settings-btn"
            type="button"
            onClick={onOpenSettings}
            aria-label="Open settings"
          >
            <span className="sidebar-labeled-button-icon" aria-hidden>
              <Settings size={14} aria-hidden />
            </span>
            <span>Settings</span>
          </button>
        </div>
      </div>
    </div>
  );
}
