import { describe, expect, it } from "vitest";
import { parseSlashInput } from "./slashCommand";
import { parseClaudeCommand } from "@/features/claude/utils/commands";

describe("slash command tokens", () => {
  it.each(["usage of this function", "quota exceeded", "explain /usage", "/tmp/file.ts", "/"])("does not turn %s into a command", (text) => {
    expect(parseSlashInput(text)).toBeNull();
  });
  it("keeps namespaced commands and multiline arguments intact", () => {
    expect(parseSlashInput(" /new:review keep\nthese arguments ")).toEqual({ name: "new:review", args: "keep\nthese arguments" });
    expect(parseClaudeCommand("/new:review")).toBeNull();
  });
  it("accepts exact aliases without swallowing similarly named skills", () => {
    expect(parseClaudeCommand("/PLUGIN")?.action).toBe("plugins");
    expect(parseClaudeCommand("/cost")?.action).toBe("usage");
    expect(parseClaudeCommand("/usage-report")).toBeNull();
    expect(parseClaudeCommand("/usage:project")).toBeNull();
  });
});
