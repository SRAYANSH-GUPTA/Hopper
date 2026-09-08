import { useEffect, type RefObject } from "react";

export function useSetupDialogFocus(ref: RefObject<HTMLDivElement | null>, open: boolean) {
  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement;
    const root = ref.current;
    if (!root) return;
    const selector = 'button:not(:disabled), input:not(:disabled), a[href], [tabindex="0"]';
    const focusable = () => Array.from(root.querySelectorAll<HTMLElement>("*")).filter((node) => node.matches(selector));
    focusable()[0]?.focus();
    const trap = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const nodes = focusable();
      if (!nodes.length) { event.preventDefault(); root.focus(); return; }
      const first = nodes[0];
      const last = nodes[nodes.length - 1];
      if (event.shiftKey && (document.activeElement === first || !root.contains(document.activeElement))) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || !root.contains(document.activeElement))) { event.preventDefault(); first.focus(); }
    };
    const keepFocus = (event: FocusEvent) => {
      if (!root.contains(event.target as Node)) {
        (focusable()[0] ?? root).focus();
      }
    };
    document.addEventListener("focusin", keepFocus);
    root.addEventListener("keydown", trap);
    return () => { document.removeEventListener("focusin", keepFocus); root.removeEventListener("keydown", trap); if (previous instanceof HTMLElement) previous.focus(); };
  }, [open, ref]);
}
