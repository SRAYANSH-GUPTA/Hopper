// @vitest-environment jsdom
import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { sendNotification } from "@/services/tauri";
import { writeTasks } from "../taskStore";
import { useTaskReminders } from "./useTaskReminders";

vi.mock("@/services/tauri", () => ({ sendNotification: vi.fn() }));

function ReminderHarness() {
  useTaskReminders();
  return null;
}

beforeEach(() => {
  window.localStorage.clear();
  vi.mocked(sendNotification).mockReset();
});

describe("useTaskReminders", () => {
  it("reminds about an unfinished task after three hours", async () => {
    const createdAt = new Date(Date.now() - (3 * 60 * 60 * 1000) - 1_000).toISOString();
    writeTasks([{
      id: "task-1",
      title: "Finish the application",
      notes: "",
      dueDate: null,
      done: false,
      createdAt,
      updatedAt: createdAt,
      completedAt: null,
    }]);

    render(<ReminderHarness />);

    await waitFor(() => expect(sendNotification).toHaveBeenCalledWith(
      "Task reminder",
      "Finish the application",
    ));
  });
});
