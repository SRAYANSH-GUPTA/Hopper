//! Turns agy transcript steps into Hopper conversation items. A tool step
//! (`RUN_COMMAND`, `VIEW_FILE`, …) only holds the tool's output; the tool name
//! and arguments come from the preceding `PLANNER_RESPONSE.tool_calls`.
use serde_json::{json, Value};
use std::collections::VecDeque;

const DETAIL_MAX_CHARS: usize = 400;

/// Tool calls planned but not yet matched to a transcript step.
#[derive(Default)]
pub(crate) struct PendingToolCalls(VecDeque<Value>);

impl PendingToolCalls {
    pub(crate) fn extend_from_planner(&mut self, step: &Value) {
        if let Some(calls) = step.get("tool_calls").and_then(Value::as_array) {
            self.0.extend(
                calls
                    .iter()
                    .filter(|call| call.get("name").and_then(Value::as_str) != Some("ask_question"))
                    .cloned(),
            );
        }
    }

    /// The call that produced a step of `step_type`. Calls planned before it
    /// that never produced a step are dropped.
    pub(crate) fn take_for_step(&mut self, step_type: &str) -> Option<Value> {
        let position = self
            .0
            .iter()
            .position(|call| call_matches_step(call, step_type))
            .or_else(|| (step_type == "GENERIC" && !self.0.is_empty()).then_some(0))?;
        self.0.drain(..position);
        self.0.pop_front()
    }
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn call_matches_step(call: &Value, step_type: &str) -> bool {
    let Some(name) = call.get("name").and_then(Value::as_str) else {
        return false;
    };
    let (name, step) = (normalize(name), normalize(step_type));
    !name.is_empty() && (name == step || step.starts_with(&name) || name.starts_with(&step))
}

fn arg<'a>(call: &'a Value, key: &str) -> Option<&'a str> {
    call.pointer(&format!("/args/{key}"))
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

fn display_path(path: &str) -> String {
    path.strip_prefix("file://").unwrap_or(path).to_string()
}

/// "VIEW_FILE" → "View file".
fn humanize_step(step_type: &str) -> String {
    let lower = step_type.replace('_', " ").to_ascii_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => "Step".into(),
    }
}

/// A step's output without agy's "Created At / Completed At" header lines.
pub(crate) fn step_output(content: &str) -> String {
    content
        .lines()
        .skip_while(|line| {
            let line = line.trim();
            line.starts_with("Created At:") || line.starts_with("Completed At:") || line.is_empty()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn prefixed_lines(text: &str, prefix: char) -> String {
    text.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn replacement_diff(path: &str, call: &Value) -> String {
    let mut chunks: Vec<(String, String)> = call
        .pointer("/args/ReplacementChunks")
        .and_then(Value::as_array)
        .map(|chunks| {
            chunks
                .iter()
                .map(|chunk| {
                    (
                        chunk.get("TargetContent").and_then(Value::as_str).unwrap_or("").to_string(),
                        chunk.get("ReplacementContent").and_then(Value::as_str).unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if chunks.is_empty() {
        chunks.push((
            arg(call, "TargetContent").unwrap_or("").to_string(),
            arg(call, "ReplacementContent").unwrap_or("").to_string(),
        ));
    }
    let header = path.trim_start_matches('/');
    let mut diff = format!("--- a/{header}\n+++ b/{header}");
    for (old, new) in chunks {
        diff.push_str("\n@@\n");
        diff.push_str(
            &[prefixed_lines(&old, '-'), prefixed_lines(&new, '+')]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    diff
}

/// The started and completed items for one agy step, given the tool call that
/// produced it (when known).
pub(crate) fn step_items(
    item_id: &str,
    turn_id: &str,
    step_type: &str,
    call: Option<&Value>,
    content: &str,
    failed: bool,
) -> (Value, Value) {
    let output = step_output(content);
    let status = if failed { "failed" } else { "completed" };
    let name = call.and_then(|call| call.get("name")).and_then(Value::as_str).unwrap_or("");
    let summary = call
        .and_then(|call| arg(call, "toolSummary"))
        .map(str::to_string);
    let tool_call = |title: String, detail: String| {
        json!({
            "type": "toolCall",
            "id": item_id,
            "turnId": turn_id,
            "tool": if name.is_empty() { step_type } else { name },
            "title": title,
            "detail": detail,
            "status": "inProgress",
        })
    };
    let started = match (name, call) {
        ("run_command", Some(call)) => json!({
            "type": "commandExecution",
            "id": item_id,
            "turnId": turn_id,
            "command": arg(call, "CommandLine").unwrap_or("command"),
            "cwd": arg(call, "Cwd").unwrap_or(""),
            "status": "inProgress",
        }),
        ("write_to_file", Some(call)) => {
            let path = display_path(arg(call, "TargetFile").unwrap_or("file"));
            let content = call.pointer("/args/CodeContent").and_then(Value::as_str).unwrap_or("");
            json!({
                "type": "fileChange",
                "id": item_id,
                "turnId": turn_id,
                "status": "inProgress",
                "changes": [{
                    "path": path,
                    "kind": "add",
                    "diff": format!(
                        "--- /dev/null\n+++ b/{}\n@@\n{}",
                        path.trim_start_matches('/'),
                        prefixed_lines(content, '+'),
                    ),
                }],
            })
        }
        ("replace_file_content" | "multi_replace_file_content", Some(call)) => {
            let path = display_path(arg(call, "TargetFile").unwrap_or("file"));
            json!({
                "type": "fileChange",
                "id": item_id,
                "turnId": turn_id,
                "status": "inProgress",
                "changes": [{ "path": path, "kind": "update", "diff": replacement_diff(&path, call) }],
            })
        }
        ("search_web", Some(call)) => json!({
            "type": "webSearch",
            "id": item_id,
            "turnId": turn_id,
            "query": arg(call, "query").or_else(|| arg(call, "Query")).unwrap_or(""),
            "status": "inProgress",
        }),
        ("view_file" | "view_file_outline", Some(call)) => tool_call(
            "Read".into(),
            display_path(arg(call, "AbsolutePath").unwrap_or("")),
        ),
        ("list_dir", Some(call)) => tool_call(
            "List".into(),
            display_path(arg(call, "DirectoryPath").unwrap_or("")),
        ),
        ("grep_search", Some(call)) => {
            let query = arg(call, "Query").unwrap_or("");
            let detail = match arg(call, "SearchPath") {
                Some(path) => format!("{query} in {}", display_path(path)),
                None => query.to_string(),
            };
            tool_call("Search".into(), detail)
        }
        ("find_by_name", Some(call)) => {
            let pattern = arg(call, "Pattern").unwrap_or("");
            let detail = match arg(call, "SearchDirectory") {
                Some(path) => format!("{pattern} in {}", display_path(path)),
                None => pattern.to_string(),
            };
            tool_call("Find files".into(), detail)
        }
        ("read_url_content", Some(call)) => tool_call(
            "Fetch".into(),
            arg(call, "Url").unwrap_or("").to_string(),
        ),
        (_, Some(call)) => tool_call(
            summary.unwrap_or_else(|| humanize_step(name)),
            arg(call, "toolAction").map(truncate).unwrap_or_default(),
        ),
        (_, None) => tool_call(humanize_step(step_type), String::new()),
    };

    let mut completed = started.clone();
    if let Some(object) = completed.as_object_mut() {
        object.insert("status".into(), json!(status));
        match object.get("type").and_then(Value::as_str).unwrap_or("") {
            "commandExecution" => {
                object.insert("aggregatedOutput".into(), json!(output));
            }
            "fileChange" | "webSearch" => {
                if failed {
                    object.insert("output".into(), json!(output));
                }
            }
            _ => {
                object.insert("output".into(), json!(output));
            }
        }
    }
    (started, completed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planner(calls: Value) -> Value {
        json!({ "type": "PLANNER_RESPONSE", "tool_calls": calls })
    }

    #[test]
    fn matches_steps_to_their_tool_calls_in_order() {
        let mut pending = PendingToolCalls::default();
        pending.extend_from_planner(&planner(json!([
            {"name": "list_dir", "args": {"DirectoryPath": "/a"}},
            {"name": "ask_question", "args": {}},
            {"name": "run_command", "args": {"CommandLine": "ls"}}
        ])));
        let call = pending.take_for_step("RUN_COMMAND").unwrap();
        assert_eq!(call["args"]["CommandLine"], "ls");
        // The unmatched list_dir planned earlier is dropped.
        assert!(pending.take_for_step("LIST_DIRECTORY").is_none());

        pending.extend_from_planner(&planner(json!([{"name": "list_dir", "args": {}}])));
        assert!(pending.take_for_step("LIST_DIRECTORY").is_some());
        pending.extend_from_planner(&planner(json!([{"name": "list_permissions", "args": {}}])));
        assert_eq!(pending.take_for_step("GENERIC").unwrap()["name"], "list_permissions");
        assert!(pending.take_for_step("VIEW_FILE").is_none());
    }

    #[test]
    fn commands_show_the_command_line_and_output() {
        let call = json!({"name": "run_command", "args": {"CommandLine": "npm test", "Cwd": "/repo"}});
        let (started, completed) = step_items(
            "tool-7",
            "turn",
            "RUN_COMMAND",
            Some(&call),
            "Created At: x\nCompleted At: y\n\nThe command completed successfully.\nOutput: ok",
            false,
        );
        assert_eq!(started["type"], "commandExecution");
        assert_eq!(started["command"], "npm test");
        assert_eq!(started["status"], "inProgress");
        assert_eq!(completed["aggregatedOutput"], "The command completed successfully.\nOutput: ok");
        assert_eq!(completed["status"], "completed");
    }

    #[test]
    fn file_tools_get_titles_paths_and_diffs() {
        let view = json!({"name": "view_file", "args": {"AbsolutePath": "file:///repo/a.ts"}});
        let (read, _) = step_items("tool-1", "turn", "VIEW_FILE", Some(&view), "", false);
        assert_eq!(read["title"], "Read");
        assert_eq!(read["detail"], "/repo/a.ts");

        let grep = json!({"name": "grep_search", "args": {"Query": "TODO", "SearchPath": "/repo"}});
        assert_eq!(step_items("tool-2", "turn", "GREP_SEARCH", Some(&grep), "", false).0["detail"], "TODO in /repo");

        let replace = json!({"name": "replace_file_content", "args": {
            "TargetFile": "/repo/a.ts",
            "ReplacementChunks": [{"TargetContent": "a", "ReplacementContent": "b"}]
        }});
        let (edit, _) = step_items("tool-3", "turn", "CODE_ACTION", Some(&replace), "", false);
        assert_eq!(edit["type"], "fileChange");
        assert_eq!(edit["changes"][0]["diff"], "--- a/repo/a.ts\n+++ b/repo/a.ts\n@@\n-a\n+b");

        let custom = json!({"name": "list_permissions", "args": {"toolSummary": "List permissions", "toolAction": "Listing permissions"}});
        let (generic, done) = step_items("tool-4", "turn", "GENERIC", Some(&custom), "grants: none", false);
        assert_eq!(generic["title"], "List permissions");
        assert_eq!(generic["detail"], "Listing permissions");
        assert_eq!(done["output"], "grants: none");
    }

    #[test]
    fn unmatched_steps_use_a_readable_step_name() {
        let (item, done) = step_items("tool-5", "turn", "ERROR_MESSAGE", None, "boom", true);
        assert_eq!(item["title"], "Error message");
        assert_eq!(done["status"], "failed");
        assert_eq!(done["output"], "boom");
    }
}
