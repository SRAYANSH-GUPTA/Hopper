import { getCurrentWindow } from "@tauri-apps/api/window";

export type DragDropPayload = {
  type: "enter" | "over" | "leave" | "drop";
  position: { x: number; y: number };
  paths?: string[];
};

export type DragDropEvent = {
  payload: DragDropPayload;
};

type Listener = (event: DragDropEvent) => void;

type SubscriptionOptions = {
  onError?: (error: unknown) => void;
};

const WEB_URL = /^https?:\/\//i;

let unlisten: (() => void) | null = null;
let listenPromise: Promise<() => void> | null = null;
const listeners = new Set<Listener>();

function emit(event: DragDropEvent) {
  for (const listener of listeners) {
    try {
      listener(event);
    } catch (error) {
      console.error("[drag-drop] listener failed", error);
    }
  }
}

function handleNativeEvent(event: DragDropEvent) {
  const payload = event.payload;
  if (payload.type !== "drop" || !payload.paths?.length) {
    emit(event);
    return;
  }
  // Web links dragged from a page arrive as URI "paths"; they are not files.
  const paths = payload.paths.filter((path) => !WEB_URL.test(path.trim()));
  emit({ payload: { ...payload, paths } });
}

function start(options?: SubscriptionOptions) {
  if (unlisten || listenPromise) {
    return;
  }
  listenPromise = getCurrentWindow()
    .onDragDropEvent((event) => {
      handleNativeEvent(event as DragDropEvent);
    }) as Promise<() => void>;
  listenPromise
    .then((handler) => {
      listenPromise = null;
      if (listeners.size === 0) {
        handler();
        return;
      }
      unlisten = handler;
    })
    .catch((error) => {
      listenPromise = null;
      options?.onError?.(error);
    });
}

function stop() {
  if (!unlisten) {
    return;
  }
  try {
    unlisten();
  } catch {
    // Ignore double-unlisten when tearing down.
  }
  unlisten = null;
}

export function subscribeWindowDragDrop(
  onEvent: Listener,
  options?: SubscriptionOptions,
) {
  listeners.add(onEvent);
  start(options);
  return () => {
    listeners.delete(onEvent);
    if (listeners.size === 0) {
      stop();
    }
  };
}
