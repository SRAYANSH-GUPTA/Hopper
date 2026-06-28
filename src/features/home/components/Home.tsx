import { useMemo } from "react";
import type {
  AccountSnapshot,
  LocalUsageSnapshot,
  RateLimitSnapshot,
} from "../../../types";
import type {
  LatestAgentRun,
  UsageMetric,
  UsageWorkspaceOption,
} from "../homeTypes";
type HomeProps = {
  onAddWorkspace: () => void;
  onAddWorkspaceFromUrl: () => void;
  latestAgentRuns: LatestAgentRun[];
  isLoadingLatestAgents: boolean;
  localUsageSnapshot: LocalUsageSnapshot | null;
  isLoadingLocalUsage: boolean;
  localUsageError: string | null;
  onRefreshLocalUsage: () => void;
  usageMetric: UsageMetric;
  onUsageMetricChange: (metric: UsageMetric) => void;
  usageWorkspaceId: string | null;
  usageWorkspaceOptions: UsageWorkspaceOption[];
  onUsageWorkspaceChange: (workspaceId: string | null) => void;
  accountRateLimits: RateLimitSnapshot | null;
  usageShowRemaining: boolean;
  accountInfo: AccountSnapshot | null;
  onSelectThread: (workspaceId: string, threadId: string) => void;
  onToggleGitSidebar?: () => void;
  gitSidebarOpen?: boolean;
};

export function Home({
  onAddWorkspace,
  onAddWorkspaceFromUrl,
}: HomeProps) {

  return (
    <div className="home home-genz">
      <div className="home-genz-bg-grid" aria-hidden />

      <div className="home-genz-hero">
        <div className="home-genz-eyebrow">AI CODING TERMINAL</div>

        <h1 className="home-genz-headline">
          <span className="home-genz-line-green">NO</span>
          <span className="home-genz-line-white">EXCUSES</span>
          <span className="home-genz-line-green">— SHIP</span>
          <span className="home-genz-line-white">FASTER.</span>
        </h1>

        <p className="home-genz-sub">
          Orchestrate AI agents across every project. Zero friction.
        </p>

        <div className="home-genz-actions">
          <button
            className="home-genz-btn home-genz-btn-primary"
            onClick={onAddWorkspace}
            data-tauri-drag-region="false"
          >
            + Add Workspace
          </button>
          <button
            className="home-genz-btn home-genz-btn-ghost"
            onClick={onAddWorkspaceFromUrl}
            data-tauri-drag-region="false"
          >
            ⤓ From URL
          </button>
        </div>
      </div>
    </div>
  );
}
