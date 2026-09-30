// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ClockStatus } from "./ClockStatus";

vi.mock("@/services/tauri", () => ({ sendNotification: vi.fn() }));

beforeEach(() => window.localStorage.clear());
afterEach(cleanup);

describe("ClockStatus", () => {
  it("shows a simple weekday and time", () => {
    render(<ClockStatus />);
    expect(screen.getByLabelText("Current time")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
