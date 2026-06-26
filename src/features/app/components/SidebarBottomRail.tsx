import Settings from "lucide-react/dist/esm/icons/settings";

type SidebarBottomRailProps = {
  sessionPercent: number | null;
  weeklyPercent: number | null;
  sessionResetLabel: string | null;
  weeklyResetLabel: string | null;
  creditsLabel: string | null;
  showWeekly: boolean;
  onOpenSettings: () => void;
  onOpenDebug: () => void;
  showDebugButton: boolean;
  showAccountSwitcher: boolean;
  accountLabel: string;
  accountActionLabel: string;
  accountDisabled: boolean;
  accountSwitching: boolean;
  accountCancelDisabled: boolean;
  onSwitchAccount: () => void;
  onCancelSwitchAccount: () => void;
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
  creditsLabel,
  showWeekly,
  onOpenSettings,
  onOpenDebug: _onOpenDebug,
  showDebugButton: _showDebugButton,
  showAccountSwitcher: _showAccountSwitcher,
  accountLabel: _accountLabel,
  accountActionLabel: _accountActionLabel,
  accountDisabled: _accountDisabled,
  accountSwitching: _accountSwitching,
  accountCancelDisabled: _accountCancelDisabled,
  onSwitchAccount: _onSwitchAccount,
  onCancelSwitchAccount: _onCancelSwitchAccount,
}: SidebarBottomRailProps) {



  return (
    <div className="sidebar-bottom-rail">
      <div className="sidebar-usage-panel">
        <div className="sidebar-usage-header">
          <div className="sidebar-usage-kicker">
            Usage
          </div>
          {creditsLabel && <div className="sidebar-usage-credits">{creditsLabel}</div>}
        </div>
        {sessionPercent === null ? (
          <div className="sidebar-usage-unavailable">
            Not reported by CLI
          </div>
        ) : (
          <div className="sidebar-usage-list">
            <UsageRow
              label="Session"
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
