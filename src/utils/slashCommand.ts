/** A command is the complete first token, never a word inside ordinary prose. */
export function parseSlashInput(text: string): { name: string; args: string } | null {
  const match = /^\/([^\s/]+)(?:\s+([\s\S]*))?$/.exec(text.trim());
  return match ? { name: match[1].toLowerCase(), args: (match[2] ?? "").trim() } : null;
}
