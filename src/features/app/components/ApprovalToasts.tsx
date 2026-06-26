import { useEffect, useMemo } from "react";
import type { ApprovalRequest, WorkspaceInfo } from "../../../types";
import { getApprovalCommandInfo } from "../../../utils/approvalRules";
import {
  ToastActions,
  ToastCard,
  ToastViewport,
} from "../../design-system/components/toast/ToastPrimitives";

type ApprovalToastsProps = {
  approvals: ApprovalRequest[];
  workspaces: WorkspaceInfo[];
  onDecision: (request: ApprovalRequest, decision: "accept" | "decline") => void;
  onRemember?: (request: ApprovalRequest, command: string[]) => void;
};

/** Map tool names to a simple icon character */
function toolIcon(toolName: string): string {
  const name = toolName.toLowerCase();
  if (name === "bash" || name === "shellexec") return "⬢";
  if (name === "write" || name === "createfile") return "✎";
  if (name === "edit" || name === "editfile" || name === "str_replace_editor") return "⊘";
  if (name === "read" || name === "readfile") return "◎";
  if (name === "glob" || name === "search" || name === "grep") return "⊙";
  if (name === "webfetch" || name === "websearch") return "◈";
  return "◆";
}

/** Pick a color class for the tool icon badge */
function toolColor(toolName: string): string {
  const name = toolName.toLowerCase();
  if (name === "bash" || name === "shellexec") return "approval-icon--shell";
  if (name === "write" || name === "createfile") return "approval-icon--write";
  if (name === "edit" || name === "str_replace_editor") return "approval-icon--edit";
  if (name === "read") return "approval-icon--read";
  return "approval-icon--default";
}

/** Extract the most useful single-line preview from params */
function primaryDetail(params: Record<string, unknown>): string | null {
  // Prefer command > file_path > description > content preview
  const candidates = ["command", "cmd", "file_path", "path", "description", "query"];
  for (const key of candidates) {
    const val = params[key];
    if (typeof val === "string" && val.trim()) {
      return val.trim();
    }
  }
  // Fall back to first string value
  for (const val of Object.values(params)) {
    if (typeof val === "string" && val.trim() && val.length < 300) {
      return val.trim();
    }
  }
  return null;
}

export function ApprovalToasts({
  approvals,
  workspaces,
  onDecision,
  onRemember,
}: ApprovalToastsProps) {
  const workspaceLabels = useMemo(
    () => new Map(workspaces.map((w) => [w.id, w.name])),
    [workspaces],
  );

  const primaryRequest = approvals[approvals.length - 1];

  // Press Enter to approve the topmost request
  useEffect(() => {
    if (!primaryRequest) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Enter") return;
      const active = document.activeElement;
      if (
        active instanceof HTMLElement &&
        (active.isContentEditable ||
          active.tagName === "INPUT" ||
          active.tagName === "TEXTAREA" ||
          active.tagName === "SELECT")
      ) return;
      e.preventDefault();
      onDecision(primaryRequest, "accept");
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onDecision, primaryRequest]);

  if (!approvals.length) return null;

  return (
    <ToastViewport className="approval-toasts" role="region" ariaLive="assertive">
      {approvals.map((request) => {
        const workspaceName = workspaceLabels.get(request.workspace_id);
        const params = request.params ?? {};
        const commandInfo = getApprovalCommandInfo(params);
        const isClaude = request.method === "claude/requestApproval";

        const tool = isClaude
          ? String(params.tool ?? "Tool")
          : request.method.replace(/^codex\/requestApproval\/?/, "") || request.method;

        const detail = primaryDetail(
          isClaude
            ? (Object.fromEntries(Object.entries(params).filter(([k]) => k !== "tool")))
            : params,
        );

        return (
          <ToastCard
            key={`${request.workspace_id}-${request.request_id}`}
            className="approval-toast-v2"
            role="alert"
          >
            {/* Header row */}
            <div className="approval-v2-header">
              <div className={`approval-v2-icon ${isClaude ? toolColor(tool) : "approval-icon--default"}`}>
                {isClaude ? toolIcon(tool) : "◆"}
              </div>
              <div className="approval-v2-header-text">
                <span className="approval-v2-label">
                  {isClaude ? "Claude needs permission" : "Approval needed"}
                </span>
                {workspaceName && (
                  <span className="approval-v2-workspace">{workspaceName}</span>
                )}
              </div>
            </div>

            {/* Tool name pill */}
            <div className="approval-v2-tool-row">
              <span className="approval-v2-tool-pill">{tool}</span>
            </div>

            {/* Primary detail (command / file path) */}
            {detail && (
              <div className="approval-v2-detail">
                <code className="approval-v2-code">{detail}</code>
              </div>
            )}

            {/* Action buttons */}
            <ToastActions className="approval-v2-actions">
              <button
                className="approval-v2-btn approval-v2-btn--decline"
                onClick={() => onDecision(request, "decline")}
              >
                Decline
              </button>
              {commandInfo && onRemember && (
                <button
                  className="approval-v2-btn approval-v2-btn--always"
                  onClick={() => onRemember(request, commandInfo.tokens)}
                  title={`Always allow: ${commandInfo.preview}`}
                >
                  Always allow
                </button>
              )}
              <button
                className="approval-v2-btn approval-v2-btn--approve"
                onClick={() => onDecision(request, "accept")}
              >
                Approve
                <kbd className="approval-v2-kbd">↵</kbd>
              </button>
            </ToastActions>
          </ToastCard>
        );
      })}
    </ToastViewport>
  );
}
