import { useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import type { ThreadSummary } from "@/types";
import { ModalShell } from "@/features/design-system/components/modal/ModalShell";

type ResumeThreadPickerProps = {
  threads: ThreadSummary[];
  currentThreadId: string | null;
  initialQuery: string;
  loading: boolean;
  paging: boolean;
  hasMore: boolean;
  onLoadMore: () => void;
  onSelect: (threadId: string) => void;
  onClose: () => void;
};

export function ResumeThreadPicker({
  threads,
  currentThreadId,
  initialQuery,
  loading,
  paging,
  hasMore,
  onLoadMore,
  onSelect,
  onClose,
}: ResumeThreadPickerProps) {
  const [query, setQuery] = useState(initialQuery);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const matches = useMemo(() => {
    const search = query.trim().toLocaleLowerCase();
    return threads.filter((thread) =>
      !search || thread.name.toLocaleLowerCase().includes(search) || thread.id.toLowerCase().includes(search),
    );
  }, [query, threads]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  useEffect(() => {
    listRef.current?.querySelectorAll("button[data-thread-id]")[selectedIndex]?.scrollIntoView?.({
      block: "nearest",
    });
  }, [selectedIndex]);

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      setSelectedIndex((index) => matches.length ? Math.min(index + 1, matches.length - 1) : 0);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setSelectedIndex((index) => Math.max(index - 1, 0));
    } else if (event.key === "Enter" && matches[selectedIndex]) {
      event.preventDefault();
      onSelect(matches[selectedIndex].id);
    }
  };

  return (
    <ModalShell className="resume-thread-picker" ariaLabel="Resume conversation" onBackdropClick={onClose}>
      <div className="ds-modal-title resume-thread-picker-title">Resume conversation</div>
      <input
        ref={inputRef}
        className="ds-modal-input resume-thread-picker-search"
        aria-label="Search conversations"
        placeholder="Search conversations by title or ID"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={handleKeyDown}
      />
      <div className="resume-thread-picker-list" ref={listRef}>
        {matches.map((thread, index) => (
          <button
            key={thread.id}
            type="button"
            data-thread-id={thread.id}
            className={`resume-thread-picker-row${index === selectedIndex ? " selected" : ""}`}
            onMouseEnter={() => setSelectedIndex(index)}
            onClick={() => onSelect(thread.id)}
          >
            <span className="resume-thread-picker-name">{thread.name}</span>
            <span className="resume-thread-picker-meta">
              {thread.id === currentThreadId ? "current" : new Date(thread.updatedAt).toLocaleDateString()}
            </span>
          </button>
        ))}
        {!loading && matches.length === 0 && (
          <div className="resume-thread-picker-empty">
            {hasMore ? "No matches in loaded conversations. Load older to search further." : "No conversations found."}
          </div>
        )}
        {loading && <div className="resume-thread-picker-empty">Loading conversations…</div>}
        {hasMore && (
          <button className="resume-thread-picker-more" type="button" onClick={onLoadMore} disabled={paging || loading}>
            {paging ? "Loading…" : "Load older conversations"}
          </button>
        )}
      </div>
    </ModalShell>
  );
}
