import type { ReactNode } from "react";

type MainTopbarProps = {
  leftNode: ReactNode;
  actionsNode?: ReactNode;
  className?: string;
};

export function MainTopbar({ leftNode, actionsNode, className }: MainTopbarProps) {
  const classNames = ["main-topbar", className].filter(Boolean).join(" ");
  const hasLeftContent = Boolean(leftNode);
  const hasActionsContent = Boolean(actionsNode);
  const timeLabel = new Date().toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
  return (
    <div className={classNames} data-tauri-drag-region>
      <div className="main-topbar-left">
        {hasLeftContent ? leftNode : <span className="main-topbar-fallback">Hopper ready</span>}
      </div>
      <div className="actions">
        {hasActionsContent ? actionsNode : <span className="main-topbar-fallback-time">{timeLabel}</span>}
      </div>
    </div>
  );
}
