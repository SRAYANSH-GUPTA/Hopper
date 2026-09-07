import { useCallback, useRef } from "react";

import type { ThreadSummary } from "../../../types";

type ThreadRow = {
  thread: ThreadSummary;
  depth: number;
};

type ThreadRowResult = {
  pinnedRows: ThreadRow[];
  unpinnedRows: ThreadRow[];
  totalRoots: number;
  hasMoreRoots: boolean;
};

type ThreadRowCacheEntry = {
  pinVersion: number;
  result: ThreadRowResult;
};

export function useThreadRows(threadParentById: Record<string, string>) {
  const cacheRef = useRef(
    new WeakMap<
      ThreadSummary[],
      Map<string, ThreadRowCacheEntry>
    >(),
  );
  const cacheParentRef = useRef(threadParentById);
  if (cacheParentRef.current !== threadParentById) {
    cacheParentRef.current = threadParentById;
    cacheRef.current = new WeakMap<
      ThreadSummary[],
      Map<string, ThreadRowCacheEntry>
    >();
  }

  const getThreadRows = useCallback(
    (
      threads: ThreadSummary[],
      isExpanded: boolean,
      workspaceId: string,
      getPinTimestamp: (workspaceId: string, threadId: string) => number | null,
      pinVersion = 0,
      activeThreadId: string | null = null,
    ): ThreadRowResult => {
      const cacheKey = `${workspaceId}:${isExpanded ? "1" : "0"}:${activeThreadId ?? ""}`;
      const threadCache = cacheRef.current.get(threads);
      const cachedEntry = threadCache?.get(cacheKey);
      if (cachedEntry && cachedEntry.pinVersion === pinVersion) {
        return cachedEntry.result;
      }

      const visibleThreads = threads.filter((thread) => {
        if (!thread.isSubagent) {
          return true;
        }
        return Boolean(threadParentById[thread.id]);
      });
      const threadIds = new Set(visibleThreads.map((thread) => thread.id));
      const childrenByParent = new Map<string, ThreadSummary[]>();
      const roots: ThreadSummary[] = [];
      const resolveVisibleParentId = (threadId: string) => {
        let current = threadParentById[threadId];
        const visited = new Set<string>([threadId]);
        while (current && !visited.has(current)) {
          if (threadIds.has(current)) {
            return current;
          }
          visited.add(current);
          current = threadParentById[current];
        }
        return null;
      };

      visibleThreads.forEach((thread) => {
        const parentId = resolveVisibleParentId(thread.id);
        if (parentId) {
          const list = childrenByParent.get(parentId) ?? [];
          list.push(thread);
          childrenByParent.set(parentId, list);
        } else {
          roots.push(thread);
        }
      });

      const pinnedRoots: ThreadSummary[] = [];
      const unpinnedRoots: ThreadSummary[] = [];
      const pinTimestampByThreadId = new Map<string, number>();

      roots.forEach((thread) => {
        const pinTime = getPinTimestamp(workspaceId, thread.id);
        if (pinTime !== null) {
          pinnedRoots.push(thread);
          pinTimestampByThreadId.set(thread.id, pinTime);
        } else {
          unpinnedRoots.push(thread);
        }
      });

      pinnedRoots.sort((a, b) => {
        const aTime = pinTimestampByThreadId.get(a.id) ?? 0;
        const bTime = pinTimestampByThreadId.get(b.id) ?? 0;
        return aTime - bTime;
      });

      const visibleRootCount = isExpanded ? unpinnedRoots.length : 3;
      const visibleRoots = unpinnedRoots.slice(0, visibleRootCount);
      if (!isExpanded && activeThreadId) {
        let activeRootId = activeThreadId;
        const visited = new Set<string>();
        while (!visited.has(activeRootId)) {
          visited.add(activeRootId);
          const parentId = resolveVisibleParentId(activeRootId);
          if (!parentId) {
            break;
          }
          activeRootId = parentId;
        }
        const activeRoot = unpinnedRoots.find((thread) => thread.id === activeRootId);
        if (activeRoot && !visibleRoots.some((thread) => thread.id === activeRootId)) {
          visibleRoots[Math.max(visibleRoots.length - 1, 0)] = activeRoot;
        }
      }

      const appendThread = (
        thread: ThreadSummary,
        depth: number,
        rows: ThreadRow[],
      ) => {
        rows.push({ thread, depth });
        const children = childrenByParent.get(thread.id) ?? [];
        children.forEach((child) => appendThread(child, depth + 1, rows));
      };

      const pinnedRows: ThreadRow[] = [];
      pinnedRoots.forEach((thread) => appendThread(thread, 0, pinnedRows));

      const unpinnedRows: ThreadRow[] = [];
      visibleRoots.forEach((thread) => appendThread(thread, 0, unpinnedRows));

      const result = {
        pinnedRows,
        unpinnedRows,
        totalRoots: unpinnedRoots.length,
        hasMoreRoots: unpinnedRoots.length > visibleRootCount,
      };
      const nextThreadCache = threadCache ?? new Map<string, ThreadRowCacheEntry>();
      nextThreadCache.set(cacheKey, {
        pinVersion,
        result,
      });
      if (!threadCache) {
        cacheRef.current.set(threads, nextThreadCache);
      }
      return result;
    },
    [threadParentById],
  );

  return { getThreadRows };
}
