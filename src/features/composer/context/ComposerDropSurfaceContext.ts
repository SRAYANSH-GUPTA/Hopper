import { createContext, useContext, type RefObject } from "react";

export type ComposerDropSurface = {
  targetRef: RefObject<HTMLDivElement | null>;
  setDragActive: (active: boolean) => void;
};

export const ComposerDropSurfaceContext = createContext<ComposerDropSurface | null>(null);

export function useComposerDropSurface() {
  return useContext(ComposerDropSurfaceContext);
}
