import type { HopperTask } from "./taskTypes";

const STORAGE_KEY = "hopper.tasks.v1";
const REMINDER_STORAGE_KEY = "hopper.task-reminders.v1";
export const TASKS_CHANGED_EVENT = "hopper:tasks-changed";
export const TASK_REMINDER_INTERVAL_MS = 3 * 60 * 60 * 1000;

function migrateTask(value: unknown): HopperTask | null {
  if (!value || typeof value !== "object") return null;
  const task = value as Record<string, unknown>;
  if (typeof task.id !== "string" || typeof task.title !== "string") return null;
  const createdAt = typeof task.createdAt === "string" ? task.createdAt : new Date().toISOString();
  const done = typeof task.done === "boolean" ? task.done : task.status === "done";
  return {
    id: task.id,
    title: task.title,
    notes: typeof task.notes === "string" ? task.notes : "",
    dueDate: typeof task.dueDate === "string" ? task.dueDate : null,
    done,
    createdAt,
    updatedAt: typeof task.updatedAt === "string" ? task.updatedAt : createdAt,
    completedAt: typeof task.completedAt === "string" ? task.completedAt : null,
  };
}

export function readTasks(): HopperTask[] {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (!stored) return [];
    const parsed = JSON.parse(stored);
    return Array.isArray(parsed)
      ? parsed.map(migrateTask).filter((task): task is HopperTask => task !== null)
      : [];
  } catch {
    return [];
  }
}

export function writeTasks(tasks: HopperTask[]): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(tasks));
    window.dispatchEvent(new Event(TASKS_CHANGED_EVENT));
  } catch {
    // A full or unavailable local store should not take down the task UI.
  }
}

export function readTaskReminderTimes(): Record<string, number> {
  try {
    const stored = window.localStorage.getItem(REMINDER_STORAGE_KEY);
    if (!stored) return {};
    const parsed = JSON.parse(stored);
    return parsed && typeof parsed === "object" ? parsed as Record<string, number> : {};
  } catch {
    return {};
  }
}

export function writeTaskReminderTimes(times: Record<string, number>): void {
  try {
    window.localStorage.setItem(REMINDER_STORAGE_KEY, JSON.stringify(times));
  } catch {
    // Reminders can safely retry during the next app session.
  }
}
