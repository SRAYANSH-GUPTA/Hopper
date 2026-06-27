import type { AppSettings } from "@/types";

export type ProviderStaticModel = {
  id: string;
  label: string;
};

export type ProviderConfig = {
  /** Stable string ID — must match the value stored in AppSettings.localProvider */
  id: string;
  /** Human-readable name shown in the toggle button */
  label: string;
  /**
   * Static model list for this provider.
   * null  → models are fetched dynamically via getModelList(workspaceId)
   * array → shown as-is, no network fetch required
   */
  staticModels: ProviderStaticModel[] | null;
  /** Fallback model id when nothing is stored in settings */
  defaultModelId: string | null;
  /** Read the currently selected model id from AppSettings */
  getModelId: (settings: AppSettings) => string | null;
  /** Produce the AppSettings patch that stores the selected model id */
  setModelId: (modelId: string) => Partial<AppSettings>;
  /**
   * Whether this provider reports session / weekly usage via the Codex
   * rate-limit API (used to decide whether to show bars or a note).
   */
  supportsUsage: boolean;
  /**
   * Optional CSS class modifier appended to the toggle button.
   * e.g. "--claude" → "sidebar-provider-toggle-btn--claude"
   */
  styleModifier?: string;
};

/**
 * Central registry of all supported providers.
 *
 * To add a new provider:
 *   1. Add an entry here.
 *   2. Add the provider id to the `LocalAgentProvider` union in src/types.ts.
 *   3. Add the Rust routing in src-tauri/src/codex/mod.rs.
 *   4. If the provider needs a new model setting key, add it to AppSettings.
 */
export const PROVIDERS: ProviderConfig[] = [
  {
    id: "codex",
    label: "Codex",
    staticModels: null, // fetched dynamically via getModelList(workspaceId)
    defaultModelId: null,
    getModelId: (s) => s.lastComposerModelId ?? null,
    setModelId: (modelId) => ({ lastComposerModelId: modelId }),
    supportsUsage: true,
  },
  {
    id: "claude",
    label: "Claude",
    staticModels: [
      { id: "claude-sonnet-4-6", label: "Sonnet 4.6" },
      { id: "claude-opus-4-6", label: "Opus 4.6" },
      { id: "claude-haiku-4-5-20251001", label: "Haiku 4.5" },
    ],
    defaultModelId: "claude-sonnet-4-6",
    getModelId: (s) => s.claudeModelId ?? "claude-sonnet-4-6",
    setModelId: (modelId) => ({ claudeModelId: modelId }),
    supportsUsage: false,
    styleModifier: "--claude",
  },
  {
    id: "antigravity",
    label: "Antigravity",
    staticModels: [
      { id: "Gemini 3.5 Flash (Medium)", label: "Gemini 3.5 Flash (Medium)" },
      { id: "Gemini 3.5 Flash (High)", label: "Gemini 3.5 Flash (High)" },
      { id: "Gemini 3.5 Flash (Low)", label: "Gemini 3.5 Flash (Low)" },
      { id: "Gemini 3.1 Pro (Low)", label: "Gemini 3.1 Pro (Low)" },
      { id: "Gemini 3.1 Pro (High)", label: "Gemini 3.1 Pro (High)" },
      { id: "Claude Sonnet 4.6 (Thinking)", label: "Claude Sonnet 4.6 (Thinking)" },
      { id: "Claude Opus 4.6 (Thinking)", label: "Claude Opus 4.6 (Thinking)" },
      { id: "GPT-OSS 120B (Medium)", label: "GPT-OSS 120B (Medium)" },
    ],
    defaultModelId: "Gemini 3.5 Flash (Medium)",
    getModelId: (s) => s.antigravityModelId ?? "Gemini 3.5 Flash (Medium)",
    setModelId: (modelId) => ({ antigravityModelId: modelId }),
    supportsUsage: false,
    styleModifier: "--antigravity",
  },
];

export const DEFAULT_PROVIDER_ID = "codex";

export const PROVIDER_MAP = new Map<string, ProviderConfig>(
  PROVIDERS.map((p) => [p.id, p]),
);
