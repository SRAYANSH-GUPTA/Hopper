import { useCallback, useState } from "react";
import { readTasks, writeTasks } from "../taskStore";
import type { HopperTask } from "../taskTypes";

type NewTask = {
  title: string;
  notes: string;
  dueDate: string | null;
};

function newId(): string {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `task-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function useTasks() {
  const [tasks, setTasks] = useState<HopperTask[]>(readTasks);

  const commit = useCallback((update: (current: HopperTask[]) => HopperTask[]) => {
    setTasks((current) => {
      const next = update(current);
      writeTasks(next);
      return next;
    });
  }, []);

  const createTask = useCallback((input: NewTask) => {
    const title = input.title.trim();
    if (!title) return null;
    const timestamp = new Date().toISOString();
    const task: HopperTask = {
      id: newId(),
      title,
      notes: input.notes.trim(),
      dueDate: input.dueDate,
      done: false,
      createdAt: timestamp,
      updatedAt: timestamp,
      completedAt: null,
    };
    commit((current) => [task, ...current]);
    return task;
  }, [commit]);

  const toggleTask = useCallback((id: string) => {
    commit((current) => current.map((task) => {
      if (task.id !== id) return task;
      const timestamp = new Date().toISOString();
      return {
        ...task,
        done: !task.done,
        completedAt: task.done ? null : timestamp,
        updatedAt: timestamp,
      };
    }));
  }, [commit]);

  const deleteTask = useCallback((id: string) => {
    commit((current) => current.filter((task) => task.id !== id));
  }, [commit]);

  return { tasks, createTask, toggleTask, deleteTask };
}
