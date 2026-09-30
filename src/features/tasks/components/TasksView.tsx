import { useState } from "react";
import { CalendarDays, Check, ClipboardPaste, Plus, Trash2 } from "lucide-react";
import { useTasks } from "../hooks/useTasks";

function formatDeadline(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  }).format(new Date(`${value}T00:00:00`));
}

export function TasksView() {
  const { tasks, createTask, toggleTask, deleteTask } = useTasks();
  const [title, setTitle] = useState("");
  const [notes, setNotes] = useState("");
  const [dueDate, setDueDate] = useState("");
  const unfinishedCount = tasks.filter((task) => !task.done).length;
  const sortedTasks = [...tasks].sort((left, right) => {
    if (left.done !== right.done) return Number(left.done) - Number(right.done);
    return right.createdAt.localeCompare(left.createdAt);
  });

  const addTask = () => {
    const task = createTask({ title, notes, dueDate: dueDate || null });
    if (!task) return;
    setTitle("");
    setNotes("");
    setDueDate("");
  };

  const pasteNotes = async () => {
    try {
      const text = await navigator.clipboard.readText();
      if (text) setNotes((current) => current ? `${current}\n${text}` : text);
    } catch {
      // The notes box still supports normal paste when clipboard access is denied.
    }
  };

  return (
    <div className="tasks-view">
      <header className="tasks-header">
        <div><span>TASKS</span><h2>What needs to be done?</h2></div>
        <span className="tasks-open-count">{unfinishedCount} left</span>
      </header>

      <section className="task-add-card" aria-label="Add task">
        <label>
          Task
          <input
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            onKeyDown={(event) => { if (event.key === "Enter") addTask(); }}
            placeholder="What do you need to do?"
            aria-label="Task heading"
          />
        </label>
        <label>
          <span className="task-field-heading">
            Notes
            <button type="button" onClick={() => void pasteNotes()}><ClipboardPaste size={13} />Paste</button>
          </span>
          <textarea
            value={notes}
            onChange={(event) => setNotes(event.target.value)}
            rows={3}
            placeholder="Add notes or paste something from your clipboard"
            aria-label="Task notes"
          />
        </label>
        <div className="task-add-footer">
          <label>
            <CalendarDays size={14} /> Deadline
            <input type="date" value={dueDate} onChange={(event) => setDueDate(event.target.value)} aria-label="Task deadline" />
          </label>
          <button type="button" className="task-add-button" disabled={!title.trim()} onClick={addTask}>
            <Plus size={15} /> Add task
          </button>
        </div>
      </section>

      <section className="task-list" aria-label="Task list">
        {sortedTasks.length === 0 ? (
          <div className="tasks-empty"><Check size={26} /><strong>No tasks yet</strong><p>Add one above to get started.</p></div>
        ) : sortedTasks.map((task) => (
          <article key={task.id} className={`task-list-item${task.done ? " is-done" : ""}`}>
            <button
              type="button"
              className="task-checkbox"
              aria-label={`${task.done ? "Mark not done" : "Mark done"}: ${task.title}`}
              onClick={() => toggleTask(task.id)}
            >
              {task.done && <Check size={14} />}
            </button>
            <div className="task-list-content">
              <strong>{task.title}</strong>
              {task.notes && <p>{task.notes}</p>}
              {task.dueDate && <time dateTime={task.dueDate}><CalendarDays size={12} />{formatDeadline(task.dueDate)}</time>}
            </div>
            <button type="button" className="task-delete" aria-label={`Delete ${task.title}`} onClick={() => deleteTask(task.id)}>
              <Trash2 size={15} />
            </button>
          </article>
        ))}
      </section>
    </div>
  );
}
