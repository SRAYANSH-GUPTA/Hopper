import { parseSlashInput } from "@utils/slashCommand";

export type ClaudeDialog = "usage" | "plugins" | "help" | "skills" | "model" | "permissions" | "status";

export const CLAUDE_COMMANDS = [
  { name: "agents", description: "List Claude’s available subagents", action: "provider" },
  { name: "clear", description: "Start a fresh conversation; keep the previous chat", action: "new" },
  { name: "compact", description: "Summarize the active Claude conversation", hint: "[instructions] · uses Claude", action: "provider" },
  { name: "context", description: "Inspect the active conversation context", action: "provider" },
  { name: "help", description: "Browse commands available in Hopper", action: "help" },
  { name: "mcp", description: "Show Claude’s MCP server status", action: "provider" },
  { name: "model", description: "Choose the Claude model for upcoming messages", hint: "[model]", action: "model" },
  { name: "new", description: "Start a new Claude conversation", action: "new" },
  { name: "permissions", description: "Choose tool permissions for upcoming messages", action: "permissions" },
  { name: "plan", description: "Toggle plan mode, or prepare a planning prompt", hint: "[on|off|prompt]", action: "plan" },
  { name: "plugins", description: "Install, enable, disable, or remove Claude plugins", action: "plugins" },
  { name: "skills", description: "Browse installed Claude skills and plugin commands", action: "skills" },
  { name: "status", description: "Show the current Claude model and permissions", action: "status" },
  { name: "usage", description: "Show conversation tokens and Claude’s usage report", action: "usage" },
] as const;

const aliases: Record<string, string> = { plugin: "plugins", models: "model", quota: "usage", cost: "usage" };

export function parseClaudeCommand(text: string) {
  const parsed = parseSlashInput(text);
  if (!parsed) return null;
  const name = aliases[parsed.name] ?? parsed.name;
  const command = CLAUDE_COMMANDS.find((entry) => entry.name === name);
  return command ? { ...command, args: parsed.args } : null;
}
