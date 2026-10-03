/** @vitest-environment jsdom */
import { createRef } from "react";
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SlashCommandOption } from "../../../services/tauri";
import { useComposerAutocompleteState } from "./useComposerAutocompleteState";

describe("useComposerAutocompleteState file mentions", () => {
  it("suggests a file even if it is already mentioned earlier in the message", () => {
    const files = ["src/App.tsx", "src/main.tsx"];
    const text = "Please review @src/App.tsx and also @";
    const selectionStart = text.length;
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;

    const { result } = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart,
        disabled: false,
        appsEnabled: true,
        skills: [],
        apps: [],
        prompts: [],
        files,
        textareaRef,
        setText: vi.fn(),
        setSelectionStart: vi.fn(),
      }),
    );

    expect(result.current.isAutocompleteOpen).toBe(true);
    expect(result.current.autocompleteMatches.map((item) => item.label)).toContain(
      "src/App.tsx",
    );
  });

  it("marks root-level file suggestions as Files group", () => {
    const files = ["AGENTS.md", "src/main.tsx"];
    const text = "@";
    const selectionStart = text.length;
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;

    const { result } = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart,
        disabled: false,
        appsEnabled: true,
        skills: [],
        apps: [],
        prompts: [],
        files,
        textareaRef,
        setText: vi.fn(),
        setSelectionStart: vi.fn(),
      }),
    );

    const rootItem = result.current.autocompleteMatches.find(
      (item) => item.label === "AGENTS.md",
    );
    expect(rootItem?.group).toBe("Files");
  });
});

describe("useComposerAutocompleteState slash commands", () => {
  it("includes built-in slash commands in alphabetical order when apps are enabled", () => {
    const text = "/";
    const selectionStart = text.length;
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;

    const { result } = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart,
        disabled: false,
        appsEnabled: true,
        skills: [],
        apps: [],
        prompts: [],
        files: [],
        textareaRef,
        setText: vi.fn(),
        setSelectionStart: vi.fn(),
      }),
    );

    const labels = result.current.autocompleteMatches.map((item) => item.label);
    expect(labels).toEqual(
      expect.arrayContaining([
        "apps",
        "compact",
        "fast",
        "fork",
        "mcp",
        "new",
        "resume",
        "review",
        "status",
      ]),
    );
    expect(labels.slice(0, 9)).toEqual([
      "apps",
      "compact",
      "fast",
      "fork",
      "mcp",
      "new",
      "resume",
      "review",
      "status",
    ]);
  });

  it("hides /apps when apps are disabled", () => {
    const text = "/";
    const selectionStart = text.length;
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;

    const { result } = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart,
        disabled: false,
        appsEnabled: false,
        skills: [],
        apps: [],
        prompts: [],
        files: [],
        textareaRef,
        setText: vi.fn(),
        setSelectionStart: vi.fn(),
      }),
    );

    const labels = result.current.autocompleteMatches.map((item) => item.label);
    expect(labels).not.toContain("apps");
    expect(labels).toEqual([
      "compact",
      "fast",
      "fork",
      "mcp",
      "new",
      "resume",
      "review",
      "status",
      "usage",
    ]);
  });

  function renderSlash(
    text: string,
    overrides: Partial<Parameters<typeof useComposerAutocompleteState>[0]> = {},
  ) {
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;
    const setText = vi.fn();
    const hook = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart: text.length,
        disabled: false,
        appsEnabled: false,
        skills: [],
        apps: [],
        prompts: [],
        files: [],
        textareaRef,
        setText,
        setSelectionStart: vi.fn(),
        ...overrides,
      }),
    );
    return { ...hook, setText };
  }

  const installed: SlashCommandOption[] = [
    {
      name: "graphify",
      description: "Build a knowledge graph",
      argumentHint: "<path>",
      kind: "skill",
      scope: "user",
      plugin: null,
      invocation: "slash",
    },
    {
      name: "deploy",
      description: "Deploy to staging",
      argumentHint: null,
      kind: "command",
      scope: "project",
      plugin: null,
      invocation: "slash",
    },
    {
      name: "compact",
      description: "Custom compact",
      argumentHint: null,
      kind: "command",
      scope: "user",
      plugin: null,
      invocation: "slash",
    },
  ];

  it("lists installed skills and commands after the built-ins", () => {
    const { result } = renderSlash("/", { activeProviderId: "claude", slashCommands: installed });
    const matches = result.current.autocompleteMatches;
    const graphify = matches.find((item) => item.label === "graphify");
    expect(graphify).toMatchObject({
      description: "Build a knowledge graph",
      hint: "<path>",
      group: "Skills",
    });
    expect(matches.find((item) => item.label === "deploy")?.group).toBe("Commands");
    expect(matches.filter((item) => item.label === "compact")).toHaveLength(1);
    expect(matches.find((item) => item.label === "compact")?.group).toBe("Slash");
    const firstInstalled = matches.findIndex((item) => item.group !== "Slash");
    expect(matches.slice(firstInstalled).every((item) => item.group !== "Slash")).toBe(true);
  });

  it("filters installed commands by the typed query", () => {
    const { result } = renderSlash("/graph", { activeProviderId: "claude", slashCommands: installed });
    expect(result.current.autocompleteMatches[0]?.label).toBe("graphify");
  });

  it("inserts slash skills after the slash", () => {
    const { result, setText } = renderSlash("/graph", {
      activeProviderId: "claude",
      slashCommands: installed,
    });
    const graphify = result.current.autocompleteMatches.find((item) => item.label === "graphify");
    act(() => result.current.applyAutocomplete(graphify!));
    expect(setText).toHaveBeenCalledWith("/graphify ");
  });

  it("inserts Codex skills as $ mentions in place of the slash", () => {
    const { result, setText } = renderSlash("/dev", {
      slashCommands: [
        {
          name: "devops-helper",
          description: null,
          argumentHint: null,
          kind: "skill",
          scope: "user",
          plugin: null,
          invocation: "mention",
        },
      ],
    });
    const skill = result.current.autocompleteMatches.find((item) => item.label === "devops-helper");
    act(() => result.current.applyAutocomplete(skill!));
    expect(setText).toHaveBeenCalledWith("$devops-helper ");
  });
});

describe("useComposerAutocompleteState $ completions", () => {
  it("separates skills and apps into grouped results", () => {
    const text = "$";
    const selectionStart = text.length;
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;

    const { result } = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart,
        disabled: false,
        appsEnabled: true,
        skills: [
          { name: "skill-a", description: "Skill A" },
          { name: "skill-b", description: "Skill B" },
        ],
        apps: [
          {
            id: "connector_calendar",
            name: "Calendar App",
            description: "Calendar app",
            isAccessible: true,
            installUrl: null,
            distributionChannel: null,
          },
          {
            id: "not-ready",
            name: "Not Ready App",
            description: "Unreleased",
            isAccessible: false,
            installUrl: "https://example.com/install",
            distributionChannel: "beta",
          },
        ],
        prompts: [],
        files: [],
        textareaRef,
        setText: vi.fn(),
        setSelectionStart: vi.fn(),
      }),
    );

    const ids = result.current.autocompleteMatches.map((item) => item.id);
    const groups = result.current.autocompleteMatches.map((item) => item.group);
    const appSuggestion = result.current.autocompleteMatches.find(
      (item) => item.id === "app:connector_calendar",
    );
    expect(ids).toEqual(["skill:skill-a", "skill:skill-b", "app:connector_calendar"]);
    expect(groups).toEqual(["Skills", "Skills", "Apps"]);
    expect(ids).not.toContain("app:not-ready");
    expect(appSuggestion?.insertText).toBe("calendar-app");
    expect(appSuggestion?.mentionPath).toBe("app://connector_calendar");
  });
});
