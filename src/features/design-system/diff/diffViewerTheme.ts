export const DIFF_VIEWER_SCROLL_CSS = `
:host {
  color-scheme: inherit;
  --diffs-bg-buffer-override: var(--surface-messages);
  --diffs-bg-context-override: var(--surface-messages);
  --diffs-bg-context-gutter-override: var(--surface-card);
  --diffs-bg-separator-override: var(--surface-card);
  --diffs-fg-number-override: var(--text-muted);
  --diffs-addition-color-override: var(--status-success, #78b887);
  --diffs-deletion-color-override: var(--status-error, #e08080);
  --diffs-bg-addition-override: color-mix(in srgb, var(--diffs-addition-color-override) 12%, var(--surface-messages));
  --diffs-bg-deletion-override: color-mix(in srgb, var(--diffs-deletion-color-override) 12%, var(--surface-messages));
}

[data-column-number],
[data-buffer],
[data-separator-wrapper],
[data-annotation-content] {
  position: static !important;
}

[data-buffer] {
  background-image: none !important;
}

[data-hover-slot] {
  left: 0 !important;
  right: auto !important;
  justify-content: flex-start !important;
}

diffs-container,
[data-diffs],
[data-diffs-header],
[data-error-wrapper] {
  position: relative !important;
  contain: layout style !important;
  isolation: isolate !important;
}

[data-diffs-header],
[data-diffs],
[data-error-wrapper] {
  --diffs-light-bg: var(--surface-messages);
  --diffs-dark-bg: var(--surface-messages);
  --diffs-bg: var(--surface-messages);
}

[data-diffs-header][data-theme-type='light'],
[data-diffs][data-theme-type='light'] {
  --diffs-bg: var(--surface-messages);
}

[data-diffs-header][data-theme-type='dark'],
[data-diffs][data-theme-type='dark'] {
  --diffs-bg: var(--surface-messages);
}

@media (prefers-color-scheme: dark) {
  [data-diffs-header]:not([data-theme-type]),
  [data-diffs]:not([data-theme-type]),
  [data-diffs-header][data-theme-type='system'],
  [data-diffs][data-theme-type='system'] {
    --diffs-bg: var(--surface-messages);
  }
}

@media (prefers-color-scheme: light) {
  [data-diffs-header]:not([data-theme-type]),
  [data-diffs]:not([data-theme-type]),
  [data-diffs-header][data-theme-type='system'],
  [data-diffs][data-theme-type='system'] {
    --diffs-bg: var(--surface-messages);
  }
}
`;

export const DIFF_VIEWER_HIGHLIGHTER_OPTIONS = {
  theme: { dark: "pierre-dark", light: "pierre-light" },
} as const;
