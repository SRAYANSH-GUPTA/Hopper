import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { ComposerDropSurfaceContext } from "@/features/composer/context/ComposerDropSurfaceContext";

type ChatPaneProps = {
  messagesNode: ReactNode;
  composerNode: ReactNode;
  className?: string;
};

export function ChatPane({ messagesNode, composerNode, className }: ChatPaneProps) {
  const paneRef = useRef<HTMLDivElement | null>(null);
  const composerRef = useRef<HTMLDivElement | null>(null);
  const [composerHeight, setComposerHeight] = useState(0);
  const [fileDragActive, setFileDragActive] = useState(false);

  useEffect(() => {
    if (!composerNode) {
      setComposerHeight(0);
      return;
    }

    const node = composerRef.current;
    if (!node) {
      return;
    }

    const updateComposerHeight = () => {
      setComposerHeight(Math.ceil(node.getBoundingClientRect().height));
    };

    updateComposerHeight();

    const observer = new ResizeObserver(() => {
      updateComposerHeight();
    });
    observer.observe(node);

    return () => {
      observer.disconnect();
    };
  }, [composerNode]);

  const paneStyle = useMemo(
    () =>
      ({
        ["--composer-overlay-height" as string]: `${composerHeight}px`,
      }) satisfies CSSProperties,
    [composerHeight],
  );
  const dropSurface = useMemo(
    () => ({ targetRef: paneRef, setDragActive: setFileDragActive }),
    [],
  );

  return (
    <ComposerDropSurfaceContext.Provider value={dropSurface}>
      <div
        ref={paneRef}
        className={`chat-pane${fileDragActive ? " is-file-drag-over" : ""}${className ? ` ${className}` : ""}`}
        style={paneStyle}
      >
        <div className="chat-pane-messages">{messagesNode}</div>
        {composerNode ? (
          <div className="chat-pane-composer" ref={composerRef}>
            {composerNode}
          </div>
        ) : null}
      </div>
    </ComposerDropSurfaceContext.Provider>
  );
}
