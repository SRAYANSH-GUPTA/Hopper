// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { DesktopLayout } from "./DesktopLayout";

vi.mock("@/features/clock/components/ClockStatus", () => ({
  ClockStatus: () => null,
}));

afterEach(cleanup);

it("keeps sidebar navigation and resizing available on home and after selecting a workspace", () => {
  const onSelectWorkspace = vi.fn();
  const onOpenSettings = vi.fn();
  const onSidebarResizeStart = vi.fn();
  const props: ComponentProps<typeof DesktopLayout> = {
    sidebarNode: <aside>
      <button onClick={onSelectWorkspace}>Existing workspace</button>
      <button onClick={onOpenSettings}>Settings</button>
    </aside>,
    updateToastNode: null,
    approvalToastsNode: null,
    errorToastsNode: null,
    homeNode: <div>Home content</div>,
    showHome: true,
    showWorkspace: false,
    topbarLeftNode: null,
    centerMode: "chat",
    preloadGitDiffs: false,
    splitChatDiffView: false,
    messagesNode: <div>Workspace messages</div>,
    gitDiffViewerNode: null,
    gitDiffPanelNode: null,
    planPanelNode: null,
    composerNode: null,
    terminalDockNode: null,
    debugPanelNode: null,
    hasActivePlan: false,
    onSidebarResizeStart,
    onChatDiffSplitPositionResizeStart: vi.fn(),
    onRightPanelResizeStart: vi.fn(),
    onPlanPanelResizeStart: vi.fn(),
  };
  const { rerender } = render(<DesktopLayout {...props} />);
  expect(screen.getByText("Home content")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(onOpenSettings).toHaveBeenCalledOnce();
  fireEvent.mouseDown(screen.getByRole("separator", { name: "Resize sidebar" }));
  expect(onSidebarResizeStart).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "Existing workspace" }));
  expect(onSelectWorkspace).toHaveBeenCalledOnce();

  rerender(<DesktopLayout {...props} showHome={false} showWorkspace />);
  expect(screen.queryByText("Home content")).toBeNull();
  expect(screen.getByText("Workspace messages")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Settings" })).toBeTruthy();
});
