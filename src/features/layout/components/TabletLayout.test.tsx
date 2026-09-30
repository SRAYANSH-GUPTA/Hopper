// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { TabletLayout } from "./TabletLayout";

afterEach(cleanup);

it("keeps projects closed until requested and returns to content from navigation", () => {
  render(<TabletLayout
    tabletNavNode={<button>Chats</button>}
    approvalToastsNode={null} updateToastNode={null} errorToastsNode={null}
    homeNode={<div>Home content</div>} showHome showWorkspace={false}
    sidebarNode={<div>Project list</div>} tabletTab="codex"
    onSidebarResizeStart={() => {}} topbarLeftNode={null}
    messagesNode={null} composerNode={null} gitDiffPanelNode={null}
    gitDiffViewerNode={null} debugPanelNode={null}
  />);
  expect(screen.queryByText("Project list")).toBeNull();
  expect(screen.getByText("Home content").closest("section")?.hidden).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Projects" }));
  expect(screen.getByText("Project list")).toBeTruthy();
  expect(screen.getByText("Home content").closest("section")?.hidden).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Chats" }));
  expect(screen.queryByText("Project list")).toBeNull();
  expect(screen.getByRole("button", { name: "Projects" }).getAttribute("aria-expanded")).toBe("false");
  expect(screen.getByText("Home content").closest("section")?.hidden).toBe(false);
});
