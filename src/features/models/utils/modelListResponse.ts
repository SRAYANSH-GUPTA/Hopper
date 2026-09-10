import type { ModelOption } from "../../../types";

const CODEX_ASTRA_MODEL: ModelOption = {
  id: "gpt-6-astra",
  model: "gpt-6-astra",
  displayName: "GPT-6 Astra",
  description: "OpenAI's most capable model for complex end-to-end work.",
  supportedReasoningEfforts: [
    { reasoningEffort: "low", description: "" },
    { reasoningEffort: "medium", description: "" },
    { reasoningEffort: "high", description: "" },
  ],
  defaultReasoningEffort: "medium",
  isDefault: false,
};

export function normalizeEffortValue(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function extractModelItems(response: unknown): unknown[] {
  if (!response || typeof response !== "object") {
    return [];
  }

  const record = response as Record<string, unknown>;
  const result =
    record.result && typeof record.result === "object"
      ? (record.result as Record<string, unknown>)
      : null;

  const resultData = result?.data;
  if (Array.isArray(resultData)) {
    return resultData;
  }

  // Some app-server versions return the catalog under `models` while the
  // response is being proxied through an adapter. Accept that equivalent
  // shape so a protocol wrapper does not make the picker appear empty.
  const resultModels = result?.models;
  if (Array.isArray(resultModels)) {
    return resultModels;
  }

  const topLevelData = record.data;
  if (Array.isArray(topLevelData)) {
    return topLevelData;
  }

  const topLevelModels = record.models;
  if (Array.isArray(topLevelModels)) {
    return topLevelModels;
  }

  return [];
}

function parseReasoningEfforts(item: Record<string, unknown>): ModelOption["supportedReasoningEfforts"] {
  const camel = item.supportedReasoningEfforts;
  if (Array.isArray(camel)) {
    return camel
      .map((effort) => {
        if (!effort || typeof effort !== "object") {
          return null;
        }
        const entry = effort as Record<string, unknown>;
        return {
          reasoningEffort: String(entry.reasoningEffort ?? entry.reasoning_effort ?? ""),
          description: String(entry.description ?? ""),
        };
      })
      .filter((effort): effort is { reasoningEffort: string; description: string } =>
        effort !== null,
      );
  }

  const snake = item.supported_reasoning_efforts;
  if (Array.isArray(snake)) {
    return snake
      .map((effort) => {
        if (!effort || typeof effort !== "object") {
          return null;
        }
        const entry = effort as Record<string, unknown>;
        return {
          reasoningEffort: String(entry.reasoningEffort ?? entry.reasoning_effort ?? ""),
          description: String(entry.description ?? ""),
        };
      })
      .filter((effort): effort is { reasoningEffort: string; description: string } =>
        effort !== null,
      );
  }

  return [];
}

export function parseModelListResponse(response: unknown): ModelOption[] {
  const items = extractModelItems(response);

  const models = items
    .map((item) => {
      if (!item || typeof item !== "object") {
        return null;
      }
      const record = item as Record<string, unknown>;
      const modelSlug = String(record.model ?? record.slug ?? record.id ?? "");
      const rawDisplayName = String(record.displayName || record.display_name || "");
      const displayName = rawDisplayName.trim().length > 0 ? rawDisplayName : modelSlug;
      return {
        id: String(record.id ?? record.model ?? ""),
        model: modelSlug,
        displayName,
        description: String(record.description ?? ""),
        supportedReasoningEfforts: parseReasoningEfforts(record),
        defaultReasoningEffort: normalizeEffortValue(
          record.defaultReasoningEffort ?? record.default_reasoning_effort,
        ),
        isDefault: Boolean(record.isDefault ?? record.is_default ?? false),
      } satisfies ModelOption;
    })
    .filter((model): model is ModelOption => model !== null);

  const astraIndex = models.findIndex(
    (model) => model.id === CODEX_ASTRA_MODEL.id || model.model === CODEX_ASTRA_MODEL.model,
  );
  if (astraIndex === -1) {
    return [...models, CODEX_ASTRA_MODEL];
  }

  models[astraIndex] = {
    ...models[astraIndex],
    supportedReasoningEfforts: CODEX_ASTRA_MODEL.supportedReasoningEfforts,
    defaultReasoningEffort: CODEX_ASTRA_MODEL.defaultReasoningEffort,
  };
  return models;
}
