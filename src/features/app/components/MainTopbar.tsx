import type { ReactNode } from "react";
import { ClockStatus } from "@/features/clock/components/ClockStatus";

type MainTopbarProps = {
  leftNode: ReactNode;
  actionsNode?: ReactNode;
  className?: string;
};

export function MainTopbar({ leftNode, actionsNode, className }: MainTopbarProps) {
  const classNames = ["main-topbar", className].filter(Boolean).join(" ");
  const hasLeftContent = Boolean(leftNode);
  const hasActionsContent = Boolean(actionsNode);
  return (
    <div className={classNames} data-tauri-drag-region>
      <div className="main-topbar-left">
        {hasLeftContent ? leftNode : <span className="main-topbar-fallback">Hopper ready</span>}
      </div>
      <div className="actions">
        {hasActionsContent && actionsNode}
        <ClockStatus />
      </div>
    </div>
  );
}
