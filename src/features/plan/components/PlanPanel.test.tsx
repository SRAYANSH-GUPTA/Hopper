// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { PlanPanel } from "./PlanPanel";

afterEach(cleanup);

describe("PlanPanel", () => {
  it("shows a waiting label while processing without a plan", () => {
    render(<PlanPanel plan={null} isProcessing />);

    expect(screen.getByText("Waiting on a plan...")).toBeTruthy();
  });

  it("shows an empty label when idle without a plan", () => {
    render(<PlanPanel plan={null} isProcessing={false} />);

    expect(screen.getByText("No active plan.")).toBeTruthy();
  });

  it("renders a generated Markdown plan", () => {
    render(
      <PlanPanel
        plan={{
          turnId: "plan-1",
          explanation: "## Proposed Plan\n\n- Implement the change",
          steps: [],
        }}
        isProcessing={false}
      />,
    );

    expect(screen.getByRole("heading", { name: "Proposed Plan" })).toBeTruthy();
    expect(screen.getByText("Implement the change")).toBeTruthy();
    expect(screen.queryByText("No active plan.")).toBeNull();
  });
});
