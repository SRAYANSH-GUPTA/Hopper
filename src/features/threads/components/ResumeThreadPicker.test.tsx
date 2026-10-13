// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ResumeThreadPicker } from "./ResumeThreadPicker";

afterEach(cleanup);

const threads = [
  { id: "thread-1", name: "First conversation", updatedAt: 1000 },
  { id: "thread-2", name: "Fix login", updatedAt: 2000 },
  { id: "subagent", name: "Agent conversation", updatedAt: 3000, isSubagent: true },
];

describe("ResumeThreadPicker", () => {
  it("searches conversations and opens the selected result", () => {
    const onSelect = vi.fn();
    render(
      <ResumeThreadPicker
        threads={threads}
        currentThreadId="thread-1"
        initialQuery=""
        loading={false}
        paging={false}
        hasMore={false}
        onLoadMore={vi.fn()}
        onSelect={onSelect}
        onClose={vi.fn()}
      />,
    );

    const search = screen.getByRole("textbox", { name: "Search conversations" });
    fireEvent.change(search, { target: { value: "login" } });
    expect(screen.getByRole("button", { name: /Fix login/ })).toBeTruthy();
    expect(screen.queryByText("Agent conversation")).toBeNull();
    fireEvent.keyDown(search, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith("thread-2");
  });

  it("loads older conversations and closes with Escape", () => {
    const onLoadMore = vi.fn();
    const onClose = vi.fn();
    render(
      <ResumeThreadPicker
        threads={threads}
        currentThreadId={null}
        initialQuery="missing"
        loading={false}
        paging={false}
        hasMore
        onLoadMore={onLoadMore}
        onSelect={vi.fn()}
        onClose={onClose}
      />,
    );

    expect(screen.getByText(/Load older to search further/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Load older conversations" }));
    expect(onLoadMore).toHaveBeenCalledOnce();
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });
});
