import type { ThreadSummary, WorkspaceInfo } from "@/types";
import type { ThreadStatusById } from "@utils/threadStatus";
import type { WorkspaceGroupSection } from "../components/sidebarTypes";

/** Rank whole projects, including their clones and worktrees, by live activity. */
export function sortWorkspacesByActivity(
  groups: WorkspaceGroupSection[],
  workspaces: WorkspaceInfo[],
  threadsByWorkspace: Record<string, ThreadSummary[]>,
  threadStatusById: ThreadStatusById,
): WorkspaceGroupSection[] {
  const children = new Map<string, string[]>();
  for (const workspace of workspaces) {
    const parentId = workspace.kind === "worktree"
      ? workspace.parentId
      : workspace.settings.cloneSourceWorkspaceId?.trim();
    if (parentId && parentId !== workspace.id) {
      children.set(parentId, [...(children.get(parentId) ?? []), workspace.id]);
    }
  }
  const activity = new Map<string, { ongoing: boolean; timestamp: number }>();
  const getActivity = (id: string) => {
    const cached = activity.get(id);
    if (cached) return cached;
    const result = { ongoing: false, timestamp: 0 };
    const visited = new Set<string>();
    const pending = [id];
    while (pending.length) {
      const current = pending.pop()!;
      if (visited.has(current)) continue;
      visited.add(current);
      for (const thread of threadsByWorkspace[current] ?? []) {
        const status = threadStatusById[thread.id];
        result.ongoing ||= Boolean(status?.isProcessing || status?.isReviewing);
        result.timestamp = Math.max(result.timestamp, thread.updatedAt ?? thread.createdAt ?? 0);
      }
      pending.push(...(children.get(current) ?? []));
    }
    activity.set(id, result);
    return result;
  };
  return groups.map((group) => ({
    ...group,
    workspaces: [...group.workspaces].sort((a, b) => {
      const left = getActivity(a.id);
      const right = getActivity(b.id);
      return Number(right.ongoing) - Number(left.ongoing)
        || right.timestamp - left.timestamp;
    }),
  }));
}
