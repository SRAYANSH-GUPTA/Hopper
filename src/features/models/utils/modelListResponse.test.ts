import { describe, expect, it } from "vitest";
import { parseModelListResponse } from "./modelListResponse";

describe("parseModelListResponse", () => {
  it("adds GPT-6 Astra with low, medium, and high reasoning efforts", () => {
    const models = parseModelListResponse({ result: { data: [] } });

    expect(models).toContainEqual({
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
    });
  });

  it("preserves the server's reasoning capabilities without duplicating its model", () => {
    const models = parseModelListResponse({
      result: {
        data: [
          {
            id: "gpt-6-astra",
            model: "gpt-6-astra",
            displayName: "GPT-6 Astra",
            supportedReasoningEfforts: [{ reasoningEffort: "max", description: "Max" }],
            defaultReasoningEffort: "max",
          },
        ],
      },
    });

    expect(models).toHaveLength(1);
    expect(models[0]?.supportedReasoningEfforts.map((effort) => effort.reasoningEffort)).toEqual([
      "max",
    ]);
    expect(models[0]?.defaultReasoningEffort).toBe("max");
  });

  it("does not inject unavailable models into a live catalog", () => {
    const models = parseModelListResponse({ data: [{ id: "available-model" }] });

    expect(models.map((model) => model.id)).toEqual(["available-model"]);
  });

  it("allows configuration screens to opt out of an offline fallback", () => {
    expect(parseModelListResponse(null, { includeFallback: false })).toEqual([]);
  });

  it("ignores invalid model entries and uses adapter slugs as ids", () => {
    const models = parseModelListResponse({ models: [null, {}, { slug: "adapter-model" }] });

    expect(models.map((model) => [model.id, model.model])).toEqual([
      ["adapter-model", "adapter-model"],
    ]);
  });

  it("uses displayName when present", () => {
    const response = {
      result: {
        data: [
          { id: "m1", model: "gpt-5.3-codex-spark", displayName: "GPT-5.3-Codex-Spark" },
        ],
      },
    };
    const [model] = parseModelListResponse(response);
    expect(model.displayName).toBe("GPT-5.3-Codex-Spark");
  });

  it("accepts model catalogs returned through adapter wrappers", () => {
    const models = parseModelListResponse({
      result: { models: [{ id: "gpt-5.3-codex", slug: "gpt-5.3-codex" }] },
    });

    expect(models[0]?.model).toBe("gpt-5.3-codex");
  });

  it("uses the raw model slug when displayName is missing", () => {
    const response = {
      result: {
        data: [{ id: "m1", model: "gpt-5.3-codex" }],
      },
    };
    const [model] = parseModelListResponse(response);
    expect(model.displayName).toBe("gpt-5.3-codex");
  });

  it("uses the raw model slug when displayName is an empty string", () => {
    const response = {
      result: {
        data: [{ id: "m1", model: "gpt-5.1-codex-mini", displayName: "" }],
      },
    };
    const [model] = parseModelListResponse(response);
    expect(model.displayName).toBe("gpt-5.1-codex-mini");
  });

  it("preserves displayName when it equals the model slug", () => {
    const response = {
      result: {
        data: [{ id: "m1", model: "gpt-5.3-codex", displayName: "gpt-5.3-codex" }],
      },
    };
    const [model] = parseModelListResponse(response);
    expect(model.displayName).toBe("gpt-5.3-codex");
  });

  it("preserves displayName when it differs from the slug", () => {
    const response = {
      result: {
        data: [
          { id: "m1", model: "gpt-5.3-codex-spark", displayName: "GPT-5.3-Codex-Spark" },
          { id: "m2", model: "gpt-5.2-codex", displayName: "gpt-5.2-codex" },
        ],
      },
    };
    const models = parseModelListResponse(response);
    expect(models[0].displayName).toBe("GPT-5.3-Codex-Spark");
    expect(models[1].displayName).toBe("gpt-5.2-codex");
  });
});
