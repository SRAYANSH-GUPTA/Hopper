// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { TasksView } from "./TasksView";

beforeEach(() => window.localStorage.clear());
afterEach(cleanup);

describe("TasksView", () => {
  it("adds a task and toggles it done", () => {
    render(<TasksView />);

    fireEvent.change(screen.getByLabelText("Task heading"), {
      target: { value: "Review contributor PR" },
    });
    fireEvent.change(screen.getByLabelText("Task notes"), {
      target: { value: "Check the failing tests" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add task" }));

    expect(screen.getByText("Review contributor PR")).toBeTruthy();
    expect(screen.getByText("Check the failing tests")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Mark done: Review contributor PR" }));
    expect(screen.getByRole("button", { name: "Mark not done: Review contributor PR" })).toBeTruthy();
    expect(screen.getByText("0 left")).toBeTruthy();
  });
});
