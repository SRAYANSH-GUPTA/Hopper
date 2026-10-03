//! Turns Claude Code stream-json blocks into Hopper conversation items: one
//! row per tool showing the actual tool and its target, thinking as reasoning,
//! and token usage in the shape of `thread/tokenUsage/updated`.
use serde_json::{json, Value};

const DETAIL_MAX_CHARS: usize = 400;
/// Context window assumed until Claude reports the real one in `result.modelUsage`.
pub(crate) const DEFAULT_CONTEXT_WINDOW: u64 = 200_000;
const EXTENDED_CONTEXT_WINDOW: u64 = 1_000_000;

fn text_field<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn truncate(value: &str) -> String {
    if value.chars().count() <= DETAIL_MAX_CHARS {
        return value.to_string();
    }
    let mut out: String = value.chars().take(DETAIL_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

fn first_text_value(input: &Value) -> String {
    input
        .as_object()
        .and_then(|object| object.values().find_map(Value::as_str))
        .map(truncate)
        .unwrap_or_default()
}

fn with_scope(target: &str, path: Option<&str>) -> String {
    match path {
        Some(path) => format!("{target} in {path}"),
        None => target.to_string(),
    }
}

fn prefixed_lines(text: &str, prefix: char) -> String {
    text.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A unified-style diff for an edit, so the row shows what changed.
fn edit_diff(path: &str, edits: &[(&str, &str)]) -> String {
    let header = path.trim_start_matches('/');
    let mut diff = format!("--- a/{header}\n+++ b/{header}");
    for (old, new) in edits {
        diff.push_str("\n@@\n");
        let removed = prefixed_lines(old, '-');
        let added = prefixed_lines(new, '+');
        diff.push_str(&[removed, added].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n"));
    }
    diff
}

fn file_change(id: &str, turn_id: &str, path: &str, kind: &str, diff: String) -> Value {
    json!({
        "type": "fileChange",
        "id": id,
        "turnId": turn_id,
        "status": "inProgress",
        "changes": [{ "path": path, "kind": kind, "diff": diff }],
    })
}

fn tool_call(id: &str, turn_id: &str, tool: &str, title: &str, detail: String) -> Value {
    json!({
        "type": "toolCall",
        "id": id,
        "turnId": turn_id,
        "tool": tool,
        "title": title,
        "detail": detail,
        "status": "inProgress",
    })
}

fn todo_lines(input: &Value) -> String {
    input
        .get("todos")
        .and_then(Value::as_array)
        .map(|todos| {
            todos
                .iter()
                .filter_map(|todo| {
                    let content = text_field(todo, "content")?;
                    let mark = match text_field(todo, "status") {
                        Some("completed") => "☑",
                        Some("in_progress") => "◐",
                        _ => "☐",
                    };
                    Some(format!("{mark} {content}"))
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// The Hopper item for a Claude tool call that has started.
pub(crate) fn tool_started_item(tool_use_id: &str, name: &str, input: &Value, turn_id: &str) -> Value {
    let id = format!("tool-{tool_use_id}");
    let file_path = text_field(input, "file_path").or_else(|| text_field(input, "notebook_path"));
    match name {
        "Bash" => json!({
            "type": "commandExecution",
            "id": id,
            "turnId": turn_id,
            "toolUseId": tool_use_id,
            "command": text_field(input, "command").unwrap_or("bash"),
            "status": "inProgress",
        }),
        "Edit" => {
            let path = file_path.unwrap_or("file");
            let old = input.get("old_string").and_then(Value::as_str).unwrap_or("");
            let new = input.get("new_string").and_then(Value::as_str).unwrap_or("");
            file_change(&id, turn_id, path, "update", edit_diff(path, &[(old, new)]))
        }
        "MultiEdit" => {
            let path = file_path.unwrap_or("file");
            let edits: Vec<(&str, &str)> = input
                .get("edits")
                .and_then(Value::as_array)
                .map(|edits| {
                    edits
                        .iter()
                        .map(|edit| {
                            (
                                edit.get("old_string").and_then(Value::as_str).unwrap_or(""),
                                edit.get("new_string").and_then(Value::as_str).unwrap_or(""),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            file_change(&id, turn_id, path, "update", edit_diff(path, &edits))
        }
        "Write" => {
            let path = file_path.unwrap_or("file");
            let content = input.get("content").and_then(Value::as_str).unwrap_or("");
            let diff = format!(
                "--- /dev/null\n+++ b/{}\n@@\n{}",
                path.trim_start_matches('/'),
                prefixed_lines(content, '+'),
            );
            file_change(&id, turn_id, path, "add", diff)
        }
        "NotebookEdit" => {
            let path = file_path.unwrap_or("notebook");
            let source = input.get("new_source").and_then(Value::as_str).unwrap_or("");
            file_change(&id, turn_id, path, "update", edit_diff(path, &[("", source)]))
        }
        "WebSearch" => json!({
            "type": "webSearch",
            "id": id,
            "turnId": turn_id,
            "query": text_field(input, "query").unwrap_or(""),
            "status": "inProgress",
        }),
        _ if name.starts_with("mcp__") => {
            let mut parts = name.trim_start_matches("mcp__").splitn(2, "__");
            let server = parts.next().unwrap_or("mcp");
            let tool = parts.next().unwrap_or("");
            json!({
                "type": "mcpToolCall",
                "id": id,
                "turnId": turn_id,
                "server": server,
                "tool": tool,
                "arguments": input,
                "status": "inProgress",
            })
        }
        "Read" => {
            let path = file_path.unwrap_or("file");
            let range = match (input.get("offset").and_then(Value::as_u64), input.get("limit").and_then(Value::as_u64)) {
                (Some(offset), Some(limit)) => format!(" · lines {offset}–{}", offset + limit),
                (Some(offset), None) => format!(" · from line {offset}"),
                _ => String::new(),
            };
            tool_call(&id, turn_id, name, "Read", format!("{path}{range}"))
        }
        "Grep" => {
            let pattern = text_field(input, "pattern").unwrap_or("");
            tool_call(&id, turn_id, name, "Search", with_scope(pattern, text_field(input, "path")))
        }
        "Glob" => {
            let pattern = text_field(input, "pattern").unwrap_or("");
            tool_call(&id, turn_id, name, "Find files", with_scope(pattern, text_field(input, "path")))
        }
        "LS" => tool_call(&id, turn_id, name, "List", text_field(input, "path").unwrap_or(".").to_string()),
        "WebFetch" => tool_call(&id, turn_id, name, "Fetch", text_field(input, "url").unwrap_or("").to_string()),
        "Task" | "Agent" => {
            let detail = text_field(input, "description")
                .or_else(|| text_field(input, "prompt"))
                .map(truncate)
                .unwrap_or_default();
            tool_call(&id, turn_id, name, "Agent", detail)
        }
        "TodoWrite" => tool_call(&id, turn_id, name, "Update todos", todo_lines(input)),
        "ExitPlanMode" => tool_call(
            &id,
            turn_id,
            name,
            "Plan",
            text_field(input, "plan").map(truncate).unwrap_or_default(),
        ),
        "AskUserQuestion" => tool_call(
            &id,
            turn_id,
            name,
            "Asked",
            input
                .pointer("/questions/0/question")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ),
        _ => tool_call(&id, turn_id, name, name, first_text_value(input)),
    }
}

/// The started item, completed with the tool's result.
pub(crate) fn tool_completed_item(started: &Value, output: &str, is_error: bool) -> Value {
    let mut item = started.clone();
    let Some(object) = item.as_object_mut() else {
        return item;
    };
    object.insert("status".into(), json!(if is_error { "failed" } else { "completed" }));
    match object.get("type").and_then(Value::as_str).unwrap_or("") {
        "commandExecution" => {
            object.insert("aggregatedOutput".into(), json!(output));
        }
        "mcpToolCall" => {
            let key = if is_error { "error" } else { "result" };
            object.insert(key.into(), json!(output));
        }
        // The diff already shows the change; only surface failures.
        "fileChange" | "webSearch" => {
            if is_error {
                object.insert("output".into(), json!(output));
            }
        }
        _ => {
            object.insert("output".into(), json!(output));
        }
    }
    item
}

/// Text of a `thinking` content block, if `block` is one.
pub(crate) fn thinking_text(block: &Value) -> Option<&str> {
    (block.get("type").and_then(Value::as_str) == Some("thinking"))
        .then(|| block.get("thinking").and_then(Value::as_str))
        .flatten()
        .filter(|text| !text.trim().is_empty())
}

/// Token counts for one request, in Codex's terms: `input` includes cached reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TokenCounts {
    pub(crate) input: u64,
    pub(crate) cached_input: u64,
    pub(crate) output: u64,
}

impl TokenCounts {
    pub(crate) fn from_usage(usage: &Value) -> Option<Self> {
        let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        usage.as_object()?;
        let cached = count("cache_read_input_tokens");
        Some(Self {
            input: count("input_tokens") + cached + count("cache_creation_input_tokens"),
            cached_input: cached,
            output: count("output_tokens"),
        })
    }

    pub(crate) fn add(self, other: Self) -> Self {
        Self {
            input: self.input + other.input,
            cached_input: self.cached_input + other.cached_input,
            output: self.output + other.output,
        }
    }

    fn to_json(self) -> Value {
        json!({
            "totalTokens": self.input + self.output,
            "inputTokens": self.input,
            "cachedInputTokens": self.cached_input,
            "outputTokens": self.output,
            "reasoningOutputTokens": 0,
        })
    }
}

/// `thread/tokenUsage/updated` params' `tokenUsage` value.
pub(crate) fn token_usage_payload(total: TokenCounts, last: TokenCounts, context_window: u64) -> Value {
    json!({
        "total": total.to_json(),
        "last": last.to_json(),
        "modelContextWindow": context_window,
    })
}

/// Context window for `model`, as reported by `result.modelUsage` when present.
pub(crate) fn context_window(model: &str, model_usage: Option<&Value>) -> u64 {
    if let Some(window) = model_usage
        .and_then(Value::as_object)
        .and_then(|models| {
            models
                .get(model)
                .or_else(|| models.values().next())
                .and_then(|usage| usage.get("contextWindow"))
                .and_then(Value::as_u64)
        })
    {
        return window;
    }
    if model.contains("[1m]") {
        EXTENDED_CONTEXT_WINDOW
    } else {
        DEFAULT_CONTEXT_WINDOW
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_rows_show_the_command_and_output() {
        let started = tool_started_item("t1", "Bash", &json!({"command": "npm test"}), "turn");
        assert_eq!(started["type"], "commandExecution");
        assert_eq!(started["command"], "npm test");
        let done = tool_completed_item(&started, "ok", false);
        assert_eq!(done["command"], "npm test");
        assert_eq!(done["aggregatedOutput"], "ok");
        assert_eq!(done["status"], "completed");
    }

    #[test]
    fn edits_become_file_changes_with_diffs() {
        let edit = tool_started_item(
            "t2",
            "Edit",
            &json!({"file_path": "src/a.ts", "old_string": "a\nb", "new_string": "c"}),
            "turn",
        );
        assert_eq!(edit["type"], "fileChange");
        assert_eq!(edit["changes"][0]["path"], "src/a.ts");
        assert_eq!(edit["changes"][0]["diff"], "--- a/src/a.ts\n+++ b/src/a.ts\n@@\n-a\n-b\n+c");
        let write = tool_started_item("t3", "Write", &json!({"file_path": "n.md", "content": "hi"}), "turn");
        assert_eq!(write["changes"][0]["kind"], "add");
        assert_eq!(write["changes"][0]["diff"], "--- /dev/null\n+++ b/n.md\n@@\n+hi");
        let failed = tool_completed_item(&write, "permission denied", true);
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["output"], "permission denied");
    }

    #[test]
    fn other_tools_get_readable_titles() {
        let read = tool_started_item("t4", "Read", &json!({"file_path": "/x/y.rs", "offset": 10, "limit": 20}), "turn");
        assert_eq!(read["type"], "toolCall");
        assert_eq!(read["title"], "Read");
        assert_eq!(read["detail"], "/x/y.rs · lines 10–30");
        let grep = tool_started_item("t5", "Grep", &json!({"pattern": "TODO", "path": "src"}), "turn");
        assert_eq!(grep["title"], "Search");
        assert_eq!(grep["detail"], "TODO in src");
        let todos = tool_started_item(
            "t6",
            "TodoWrite",
            &json!({"todos": [{"content": "A", "status": "completed"}, {"content": "B", "status": "pending"}]}),
            "turn",
        );
        assert_eq!(todos["detail"], "☑ A\n☐ B");
        let mcp = tool_started_item("t7", "mcp__github__create_issue", &json!({"title": "x"}), "turn");
        assert_eq!(mcp["type"], "mcpToolCall");
        assert_eq!(mcp["server"], "github");
        assert_eq!(mcp["tool"], "create_issue");
        let unknown = tool_started_item("t8", "Skill", &json!({"skill": "graphify"}), "turn");
        assert_eq!(unknown["title"], "Skill");
        assert_eq!(unknown["detail"], "graphify");
        assert_eq!(tool_completed_item(&unknown, "done", false)["output"], "done");
    }

    #[test]
    fn reads_thinking_blocks() {
        assert_eq!(thinking_text(&json!({"type": "thinking", "thinking": "hmm"})), Some("hmm"));
        assert_eq!(thinking_text(&json!({"type": "thinking", "thinking": "  "})), None);
        assert_eq!(thinking_text(&json!({"type": "text", "text": "hi"})), None);
    }

    #[test]
    fn converts_usage_to_codex_token_counts() {
        let counts = TokenCounts::from_usage(&json!({
            "input_tokens": 10, "cache_read_input_tokens": 1000,
            "cache_creation_input_tokens": 200, "output_tokens": 50
        }))
        .unwrap();
        assert_eq!(counts, TokenCounts { input: 1210, cached_input: 1000, output: 50 });
        let payload = token_usage_payload(counts.add(counts), counts, 200_000);
        assert_eq!(payload["total"]["totalTokens"], 2520);
        assert_eq!(payload["last"]["cachedInputTokens"], 1000);
        assert_eq!(payload["modelContextWindow"], 200_000);
        assert!(TokenCounts::from_usage(&json!(null)).is_none());
    }

    #[test]
    fn prefers_reported_context_window() {
        let usage = json!({"claude-opus-4-1": {"contextWindow": 500000}});
        assert_eq!(context_window("claude-opus-4-1", Some(&usage)), 500_000);
        assert_eq!(context_window("claude-sonnet-4-6[1m]", None), EXTENDED_CONTEXT_WINDOW);
        assert_eq!(context_window("claude-sonnet-4-6", None), DEFAULT_CONTEXT_WINDOW);
    }
}
