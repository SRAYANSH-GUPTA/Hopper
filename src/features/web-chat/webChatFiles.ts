// Hands local files from the assistant panel (e.g. a finished download) to the
// main composer, which attaches them to the active Hopper chat draft.
type ComposerFileListener = (path: string) => void;

const listeners = new Set<ComposerFileListener>();

/** Attaches `path` to the active Hopper chat draft. */
export function offerComposerFile(path: string): void {
  for (const listener of listeners) listener(path);
}

export function subscribeComposerFileOffers(listener: ComposerFileListener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
