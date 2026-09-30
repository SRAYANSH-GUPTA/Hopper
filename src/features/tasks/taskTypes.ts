export type HopperTask = {
  id: string;
  title: string;
  notes: string;
  dueDate: string | null;
  done: boolean;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
};
