import { useEffect } from "react";
import { sendNotification } from "@/services/tauri";
import {
  readTaskReminderTimes,
  readTasks,
  TASK_REMINDER_INTERVAL_MS,
  TASKS_CHANGED_EVENT,
  writeTaskReminderTimes,
} from "../taskStore";

const MAX_TIMEOUT_MS = 2_147_000_000;

export function useTaskReminders() {
  useEffect(() => {
    let timeout = 0;

    const schedule = () => {
      window.clearTimeout(timeout);
      const tasks = readTasks().filter((task) => !task.done);
      if (tasks.length === 0) return;

      const now = Date.now();
      const reminderTimes = readTaskReminderTimes();
      const dueTasks = tasks.filter((task) => {
        const createdAt = Date.parse(task.createdAt);
        const lastReminder = reminderTimes[task.id] ?? (Number.isNaN(createdAt) ? now : createdAt);
        return now - lastReminder >= TASK_REMINDER_INTERVAL_MS;
      });

      if (dueTasks.length > 0) {
        for (const task of dueTasks) reminderTimes[task.id] = now;
        writeTaskReminderTimes(reminderTimes);
        const names = dueTasks.slice(0, 3).map((task) => task.title).join(", ");
        const remaining = dueTasks.length - 3;
        const body = remaining > 0 ? `${names}, and ${remaining} more` : names;
        void sendNotification(
          dueTasks.length === 1 ? "Task reminder" : `${dueTasks.length} task reminders`,
          body,
        );
      }

      const nextDelay = tasks.reduce((soonest, task) => {
        const createdAt = Date.parse(task.createdAt);
        const lastReminder = reminderTimes[task.id] ?? (Number.isNaN(createdAt) ? now : createdAt);
        return Math.min(soonest, Math.max(1_000, TASK_REMINDER_INTERVAL_MS - (now - lastReminder)));
      }, MAX_TIMEOUT_MS);
      timeout = window.setTimeout(schedule, nextDelay);
    };

    window.addEventListener(TASKS_CHANGED_EVENT, schedule);
    schedule();
    return () => {
      window.clearTimeout(timeout);
      window.removeEventListener(TASKS_CHANGED_EVENT, schedule);
    };
  }, []);
}
