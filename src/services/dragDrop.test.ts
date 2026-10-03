import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DragDropEvent } from "./dragDrop";

let nativeHandler: ((event: DragDropEvent) => void) | null = null;

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onDragDropEvent: (handler: (event: DragDropEvent) => void) => {
      nativeHandler = handler;
      return Promise.resolve(() => {});
    },
  }),
}));

async function subscribe() {
  const { subscribeWindowDragDrop } = await import("./dragDrop");
  const events: DragDropEvent[] = [];
  const unsubscribe = subscribeWindowDragDrop((event) => events.push(event));
  return { events, unsubscribe };
}

function drop(paths: string[]): DragDropEvent {
  return { payload: { type: "drop", position: { x: 4, y: 8 }, paths } };
}

describe("dragDrop", () => {
  beforeEach(() => {
    vi.resetModules();
    nativeHandler = null;
  });

  it("drops web link pseudo-paths and passes other events through", async () => {
    const { events, unsubscribe } = await subscribe();
    const over: DragDropEvent = { payload: { type: "over", position: { x: 1, y: 2 } } };
    nativeHandler?.(over);
    nativeHandler?.(drop(["https://example.com/page", "/home/me/file.txt"]));
    expect(events[0]).toBe(over);
    expect(events[1].payload.paths).toEqual(["/home/me/file.txt"]);
    unsubscribe();
  });
});
