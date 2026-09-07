import { describe, expect, it } from "vitest";
import type { WorkspaceInfo } from "@/types";
import { sortWorkspacesByActivity } from "./workspaceActivity";

const workspace = (id: string, extra: Partial<WorkspaceInfo> = {}): WorkspaceInfo => ({
  id, name: id, path: `/tmp/${id}`, connected: true,
  settings: { sidebarCollapsed: false }, ...extra,
});

describe("sortWorkspacesByActivity", () => {
  it("puts ongoing projects before recent projects and scans every thread", () => {
    const workspaces = [workspace("old"), workspace("recent"), workspace("running")];
    const groups = [{ id: null, name: "Projects", workspaces }];
    const result = sortWorkspacesByActivity(groups, workspaces, {
      old: [{ id: "old", name: "Old", updatedAt: 10 }],
      recent: [{ id: "first", name: "First", updatedAt: 1 }, { id: "new", name: "New", updatedAt: 100 }],
      running: [{ id: "active", name: "Active", updatedAt: 2 }],
    }, { active: { isProcessing: true } });
    expect(result[0].workspaces.map((item) => item.id)).toEqual(["running", "recent", "old"]);
    expect(groups[0].workspaces.map((item) => item.id)).toEqual(["old", "recent", "running"]);
  });

  it("includes nested clone and worktree activity and keeps group boundaries", () => {
    const root = workspace("root");
    const recent = workspace("recent");
    const clone = workspace("clone", { settings: { sidebarCollapsed: false, cloneSourceWorkspaceId: "root" } });
    const tree = workspace("tree", { kind: "worktree", parentId: "clone" });
    const groups = [{ id: "first", name: "First", workspaces: [recent, root] }, { id: "second", name: "Second", workspaces: [workspace("other")] }];
    const result = sortWorkspacesByActivity(groups, [root, recent, clone, tree], {
      recent: [{ id: "new", name: "New", updatedAt: 100 }],
      tree: [{ id: "review", name: "Review", updatedAt: 2 }],
    }, { review: { isReviewing: true } });
    expect(result[0].workspaces.map((item) => item.id)).toEqual(["root", "recent"]);
    expect(result[1]).toEqual(groups[1]);
  });

  it("keeps the existing order for projects with equal activity", () => {
    const workspaces = [workspace("z"), workspace("a")];
    const groups = [{ id: null, name: "Projects", workspaces }];
    expect(sortWorkspacesByActivity(groups, workspaces, {}, {})[0].workspaces).toEqual(workspaces);
  });
});
