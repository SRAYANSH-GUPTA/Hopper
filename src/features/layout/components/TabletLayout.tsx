import { useState, type MouseEvent, type ReactNode } from "react";
import PanelLeft from "lucide-react/dist/esm/icons/panel-left";
import { MainTopbar } from "../../app/components/MainTopbar";
import { ChatPane } from "./ChatPane";

type TabletLayoutProps = {
  tabletNavNode: ReactNode;
  approvalToastsNode: ReactNode;
  updateToastNode: ReactNode;
  errorToastsNode: ReactNode;
  homeNode: ReactNode;
  showHome: boolean;
  showWorkspace: boolean;
  sidebarNode: ReactNode;
  tabletTab: "projects" | "codex" | "git" | "log";
  onSidebarResizeStart: (event: MouseEvent<HTMLDivElement>) => void;
  topbarLeftNode: ReactNode;
  topbarActionsNode?: ReactNode;
  messagesNode: ReactNode;
  composerNode: ReactNode;
  gitDiffPanelNode: ReactNode;
  gitDiffViewerNode: ReactNode;
  debugPanelNode: ReactNode;
};

export function TabletLayout({
  tabletNavNode,
  approvalToastsNode,
  updateToastNode,
  errorToastsNode,
  homeNode,
  showHome,
  showWorkspace,
  sidebarNode,
  tabletTab,
  topbarLeftNode,
  topbarActionsNode,
  messagesNode,
  composerNode,
  gitDiffPanelNode,
  gitDiffViewerNode,
  debugPanelNode,
}: TabletLayoutProps) {
  const [projectsOpen, setProjectsOpen] = useState(false);

  return (
    <>
      <div className="tablet-toolbar">
        <button
          type="button"
          className="tablet-nav-item"
          aria-expanded={projectsOpen}
          aria-controls="tablet-projects"
          onClick={() => setProjectsOpen((open) => !open)}
        >
          <PanelLeft size={18} aria-hidden />
          <span>{projectsOpen ? "Close projects" : "Projects"}</span>
        </button>
        <div onClick={() => setProjectsOpen(false)}>{tabletNavNode}</div>
      </div>
      {projectsOpen && (
        <div id="tablet-projects" className="tablet-projects">{sidebarNode}</div>
      )}
      <section className="tablet-main" hidden={projectsOpen}>
        {approvalToastsNode}
        {updateToastNode}
        {errorToastsNode}
        {showHome && homeNode}
        {showWorkspace && (
          <>
            <MainTopbar
              leftNode={topbarLeftNode}
              actionsNode={topbarActionsNode}
              className="tablet-topbar"
            />
            {tabletTab === "codex" && (
              <div className="content tablet-content">
                <ChatPane messagesNode={messagesNode} composerNode={composerNode} />
              </div>
            )}
            {tabletTab === "git" && (
              <div className="tablet-git">
                {gitDiffPanelNode}
                <div className="tablet-git-viewer">{gitDiffViewerNode}</div>
              </div>
            )}
            {tabletTab === "log" && debugPanelNode}
          </>
        )}
      </section>
    </>
  );
}
