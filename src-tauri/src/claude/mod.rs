use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::backend::events::{AppServerEvent, EventSink};
use crate::types::LocalAgentProvider;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone)]
struct ClaudeThread {
    thread_id: String,
    session_id: Option<String>,
    created_at: i64,
}

pub(crate) struct ClaudeState {
    /// workspace_id -> (thread_id -> ClaudeThread)
    threads: Mutex<HashMap<String, HashMap<String, ClaudeThread>>>,
}

impl ClaudeState {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            threads: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) async fn new_thread(&self, workspace_id: &str) -> String {
        let thread_id = Uuid::new_v4().to_string();
        let thread = ClaudeThread {
            thread_id: thread_id.clone(),
            session_id: None,
            created_at: now_ms(),
        };
        self.threads
            .lock()
            .await
            .entry(workspace_id.to_string())
            .or_default()
            .insert(thread_id.clone(), thread);
        thread_id
    }

    pub(crate) async fn get_session_id(&self, workspace_id: &str, thread_id: &str) -> Option<String> {
        self.threads
            .lock()
            .await
            .get(workspace_id)?
            .get(thread_id)?
            .session_id
            .clone()
    }

    pub(crate) async fn set_session_id(&self, workspace_id: &str, thread_id: &str, session_id: String) {
        let mut threads = self.threads.lock().await;
        if let Some(workspace_threads) = threads.get_mut(workspace_id) {
            if let Some(thread) = workspace_threads.get_mut(thread_id) {
                thread.session_id = Some(session_id);
            }
        }
    }

    pub(crate) async fn list_threads(&self, workspace_id: &str) -> Vec<ClaudeThread> {
        self.threads
            .lock()
            .await
            .get(workspace_id)
            .map(|map| map.values().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) async fn get_thread(&self, workspace_id: &str, thread_id: &str) -> Option<ClaudeThread> {
        self.threads
            .lock()
            .await
            .get(workspace_id)?
            .get(thread_id)
            .cloned()
    }

    pub(crate) async fn ensure_thread(&self, workspace_id: &str, thread_id: &str) {
        let mut threads = self.threads.lock().await;
        threads
            .entry(workspace_id.to_string())
            .or_default()
            .entry(thread_id.to_string())
            .or_insert_with(|| ClaudeThread {
                thread_id: thread_id.to_string(),
                session_id: None,
                created_at: now_ms(),
            });
    }
}

pub(crate) async fn is_claude_mode(app_settings: &Mutex<crate::types::AppSettings>) -> bool {
    matches!(app_settings.lock().await.local_provider, LocalAgentProvider::Claude)
}

pub(crate) fn connect_workspace_claude<E: EventSink>(workspace_id: &str, event_sink: E) {
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.to_string(),
        message: json!({
            "method": "agent/connected",
            "params": { "workspaceId": workspace_id }
        }),
    });
}

pub(crate) async fn start_thread_claude<E: EventSink>(
    claude_state: &Arc<ClaudeState>,
    workspace_id: &str,
    event_sink: E,
) -> Result<Value, String> {
    let thread_id = claude_state.new_thread(workspace_id).await;
    let created_at = now_ms();

    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.to_string(),
        message: json!({
            "method": "thread/started",
            "params": {
                "thread": {
                    "id": thread_id,
                    "status": { "type": "idle" },
                    "createdAt": created_at,
                    "preview": null,
                }
            }
        }),
    });

    Ok(json!({
        "thread": {
            "id": thread_id,
            "status": { "type": "idle" },
            "createdAt": created_at,
        }
    }))
}

pub(crate) async fn list_threads_claude(
    claude_state: &Arc<ClaudeState>,
    workspace_id: &str,
) -> Result<Value, String> {
    let threads = claude_state.list_threads(workspace_id).await;
    let mut items: Vec<Value> = threads
        .iter()
        .map(|t| {
            json!({
                "id": t.thread_id,
                "status": { "type": "idle" },
                "createdAt": t.created_at,
                "preview": null,
            })
        })
        .collect();
    // Most recent first
    items.sort_by(|a, b| {
        let ts_a = a.get("createdAt").and_then(|v| v.as_i64()).unwrap_or(0);
        let ts_b = b.get("createdAt").and_then(|v| v.as_i64()).unwrap_or(0);
        ts_b.cmp(&ts_a)
    });

    Ok(json!({
        "result": {
            "data": items,
            "nextCursor": null,
        }
    }))
}

pub(crate) async fn read_thread_claude(
    claude_state: &Arc<ClaudeState>,
    workspace_id: &str,
    thread_id: &str,
) -> Result<Value, String> {
    let thread = claude_state.get_thread(workspace_id, thread_id).await;
    let (created_at, session_id) = thread
        .map(|t| (t.created_at, t.session_id))
        .unwrap_or_else(|| (now_ms(), None));

    Ok(json!({
        "result": {
            "thread": {
                "id": thread_id,
                "status": { "type": "idle" },
                "createdAt": created_at,
                "preview": null,
                "sessionId": session_id,
            },
            "items": []
        }
    }))
}

/// Resolve the absolute path of the `claude` binary by asking a login shell.
/// GUI apps on macOS/Linux don't inherit the full user PATH (nvm, homebrew,
/// npm global bins, etc.), so a bare `Command::new("claude")` silently fails.
async fn resolve_claude_bin() -> String {
    // Ask a login shell for the resolved path so we pick up nvm, homebrew, etc.
    let out = Command::new("/bin/sh")
        .args(["-lc", "which claude 2>/dev/null || command -v claude 2>/dev/null"])
        .output()
        .await;

    if let Ok(o) = out {
        let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !path.is_empty() {
            return path;
        }
    }

    // Common fallback locations
    for candidate in &[
        "/usr/local/bin/claude",
        "/opt/homebrew/bin/claude",
    ] {
        if std::path::Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }

    // Check HOME-relative paths
    if let Ok(home) = std::env::var("HOME") {
        for suffix in &[".npm/bin/claude", ".local/bin/claude", ".yarn/bin/claude"] {
            let p = format!("{home}/{suffix}");
            if std::path::Path::new(&p).exists() {
                return p;
            }
        }
    }

    // Last resort — hope it's on PATH
    "claude".to_string()
}

pub(crate) async fn send_message_claude<E: EventSink + 'static>(
    claude_state: Arc<ClaudeState>,
    workspace_id: String,
    workspace_cwd: String,
    thread_id: String,
    text: String,
    model_id: Option<String>,
    event_sink: E,
) -> Result<Value, String> {
    // Ensure thread exists in state
    claude_state.ensure_thread(&workspace_id, &thread_id).await;

    let session_id = claude_state.get_session_id(&workspace_id, &thread_id).await;
    let turn_id = Uuid::new_v4().to_string();

    // Emit thread/status/changed to running
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.clone(),
        message: json!({
            "method": "thread/status/changed",
            "params": {
                "threadId": thread_id,
                "status": { "type": "running" }
            }
        }),
    });

    // Emit turn/started
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.clone(),
        message: json!({
            "method": "turn/started",
            "params": {
                "threadId": thread_id,
                "turn": { "id": turn_id, "threadId": thread_id }
            }
        }),
    });

    // Emit user message item
    let user_item_id = Uuid::new_v4().to_string();
    let user_item = json!({
        "type": "userMessage",
        "id": user_item_id,
        "turnId": turn_id,
        "content": [{ "type": "text", "text": text }]
    });
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.clone(),
        message: json!({
            "method": "item/started",
            "params": { "threadId": thread_id, "item": user_item.clone() }
        }),
    });
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.clone(),
        message: json!({
            "method": "item/completed",
            "params": { "threadId": thread_id, "item": user_item }
        }),
    });

    // Resolve the claude binary (GUI apps don't have the full shell PATH)
    let claude_bin = resolve_claude_bin().await;

    // Build claude command
    let mut cmd = Command::new(&claude_bin);
    cmd.arg("--output-format").arg("stream-json");
    // --verbose is required by the CLI when using --output-format=stream-json with -p.
    // In stream-json mode all output (including verbose info) is valid JSON lines.
    cmd.arg("--verbose");
    if let Some(ref sid) = session_id {
        cmd.arg("--resume").arg(sid);
    }
    let resolved_model = model_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("claude-sonnet-4-6");
    cmd.arg("--model").arg(resolved_model);
    cmd.arg("-p").arg(&text);
    if !workspace_cwd.is_empty() {
        cmd.current_dir(&workspace_cwd);
    }
    // Close stdin — without this, claude may block waiting for input.
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    // Pipe stderr so we can consume it — leaving it un-read can deadlock
    // the process when the OS pipe buffer fills up.
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "Failed to spawn claude (tried '{claude_bin}'). \
             Make sure claude-code is installed: npm install -g @anthropic-ai/claude-code\n{e}"
        )
    })?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    // Consume stderr in a background task to prevent pipe buffer deadlock.
    // Errors are printed to the host process stderr for debugging.
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("[claude stderr] {line}");
            }
        });
    }

    // Clone before the async move so originals are available for the return value.
    let thread_id_ret = thread_id.clone();
    let turn_id_ret = turn_id.clone();

    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut new_session_id: Option<String> = None;
        // tool_use_id -> item_id (for tool_result matching)
        let mut tool_item_ids: HashMap<String, String> = HashMap::new();
        // message_id -> accumulated text (deduplicates streaming assistant events)
        let mut message_texts: HashMap<String, String> = HashMap::new();
        // message IDs that have already had item/started emitted
        let mut started_messages: HashSet<String> = HashSet::new();
        // Track whether we received a result/error event so we can emit a
        // fallback completion if Claude exits without one.
        let mut turn_completed = false;

        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            // Log every raw line for debugging — visible in the terminal that
            // launched the Tauri app.
            eprintln!("[claude stdout] {line}");

            let event: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[claude parse error] {e}: {line}");
                    continue;
                }
            };

            let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match event_type {
                "system" => {
                    if let Some(sid) = event.get("session_id").and_then(|s| s.as_str()) {
                        new_session_id = Some(sid.to_string());
                    }
                }
                "assistant" => {
                    // Claude stream-json emits multiple type:"assistant" events with the
                    // SAME message id as content streams in. We must use the message's own
                    // id (e.g. "msg_016af8…") as the stable item_id so the frontend can
                    // match item/started → item/agentMessage/delta → item/completed.
                    let msg_obj = event.get("message");
                    let msg_id = msg_obj
                        .and_then(|m| m.get("id"))
                        .and_then(|id| id.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| Uuid::new_v4().to_string());

                    // Collect full text and process tool_use blocks from this snapshot.
                    let mut full_text = String::new();
                    if let Some(content_arr) = msg_obj
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        for block in content_arr {
                            let block_type = block.get("type").and_then(|t| t.as_str());
                            match block_type {
                                Some("text") => {
                                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                        full_text.push_str(t);
                                    }
                                }
                                Some("tool_use") => {
                                    // Tool-use blocks live inside assistant message content in
                                    // stream-json format (not as top-level type:"tool_use" events).
                                    let tool_use_id = block
                                        .get("id")
                                        .and_then(|id| id.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    if !tool_use_id.is_empty()
                                        && !tool_item_ids.contains_key(&tool_use_id)
                                    {
                                        let tool_name = block
                                            .get("name")
                                            .and_then(|n| n.as_str())
                                            .unwrap_or("unknown")
                                            .to_string();
                                        let tool_input =
                                            block.get("input").cloned().unwrap_or(json!({}));
                                        let item_id = format!("tool-{tool_use_id}");
                                        tool_item_ids
                                            .insert(tool_use_id.clone(), item_id.clone());
                                        let command_str = serde_json::to_string(&tool_input)
                                            .unwrap_or_default();
                                        event_sink.emit_app_server_event(AppServerEvent {
                                            workspace_id: workspace_id.clone(),
                                            message: json!({
                                                "method": "item/started",
                                                "params": {
                                                    "threadId": thread_id,
                                                    "item": {
                                                        "type": "commandExecution",
                                                        "id": item_id,
                                                        "turnId": turn_id,
                                                        "toolUseId": tool_use_id,
                                                        "name": tool_name,
                                                        "command": command_str,
                                                        "status": "running"
                                                    }
                                                }
                                            }),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    // Only create an agentMessage item if this message has text.
                    // Messages that only contain tool_use blocks must NOT get an
                    // item/started — they would show as empty boxes in the UI.
                    if !full_text.is_empty() {
                        // Emit item/started the first time we see text for this message.
                        if !started_messages.contains(&msg_id) {
                            started_messages.insert(msg_id.clone());
                            message_texts.insert(msg_id.clone(), String::new());
                            event_sink.emit_app_server_event(AppServerEvent {
                                workspace_id: workspace_id.clone(),
                                message: json!({
                                    "method": "item/started",
                                    "params": {
                                        "threadId": thread_id,
                                        "item": {
                                            "type": "agentMessage",
                                            "id": msg_id,
                                            "turnId": turn_id,
                                            "text": ""
                                        }
                                    }
                                }),
                            });
                        }

                        // Emit only the *new* portion of text as a delta so the frontend
                        // can incrementally append rather than re-render the whole message.
                        let prev_len = message_texts.get(&msg_id).map(|s| s.len()).unwrap_or(0);
                        if full_text.len() > prev_len {
                            let delta = full_text[prev_len..].to_string();
                            event_sink.emit_app_server_event(AppServerEvent {
                                workspace_id: workspace_id.clone(),
                                message: json!({
                                    "method": "item/agentMessage/delta",
                                    "params": {
                                        "threadId": thread_id,
                                        "itemId": msg_id,
                                        "delta": delta
                                    }
                                }),
                            });
                            message_texts.insert(msg_id.clone(), full_text);
                        }
                    }
                }
                // Note: top-level "tool_use" events don't occur in stream-json mode —
                // tool_use blocks arrive inside type:"assistant" content (handled above).
                // This arm is left as a no-op safety net.
                "tool_use" => {}
                // type:"user" events from Claude contain tool_result blocks.
                "user" => {
                    if let Some(content_arr) = event
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        for block in content_arr {
                            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                                let tool_use_id = block
                                    .get("tool_use_id")
                                    .and_then(|id| id.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let item_id = tool_item_ids
                                    .get(&tool_use_id)
                                    .cloned()
                                    .unwrap_or_else(|| format!("tool-{tool_use_id}"));
                                let output = match block.get("content") {
                                    Some(Value::String(s)) => s.clone(),
                                    Some(Value::Array(arr)) => {
                                        // content blocks array — extract text blocks
                                        arr.iter()
                                            .filter_map(|b| {
                                                if b.get("type").and_then(|t| t.as_str())
                                                    == Some("text")
                                                {
                                                    b.get("text")
                                                        .and_then(|t| t.as_str())
                                                        .map(|s| s.to_string())
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    }
                                    Some(v) => serde_json::to_string(v).unwrap_or_default(),
                                    None => String::new(),
                                };
                                event_sink.emit_app_server_event(AppServerEvent {
                                    workspace_id: workspace_id.clone(),
                                    message: json!({
                                        "method": "item/completed",
                                        "params": {
                                            "threadId": thread_id,
                                            "item": {
                                                "type": "commandExecution",
                                                "id": item_id,
                                                "turnId": turn_id,
                                                "toolUseId": tool_use_id,
                                                "status": "completed",
                                                "output": output
                                            }
                                        }
                                    }),
                                });
                            }
                        }
                    }
                }
                "result" => {
                    if let Some(sid) = event.get("session_id").and_then(|s| s.as_str()) {
                        new_session_id = Some(sid.to_string());
                    }
                    turn_completed = true;

                    // Complete all agent message items that were streamed.
                    for (msg_id, text) in &message_texts {
                        event_sink.emit_app_server_event(AppServerEvent {
                            workspace_id: workspace_id.clone(),
                            message: json!({
                                "method": "item/completed",
                                "params": {
                                    "threadId": thread_id,
                                    "item": {
                                        "type": "agentMessage",
                                        "id": msg_id,
                                        "turnId": turn_id,
                                        "text": text
                                    }
                                }
                            }),
                        });
                    }

                    event_sink.emit_app_server_event(AppServerEvent {
                        workspace_id: workspace_id.clone(),
                        message: json!({
                            "method": "thread/status/changed",
                            "params": {
                                "threadId": thread_id,
                                "status": { "type": "idle" }
                            }
                        }),
                    });
                    event_sink.emit_app_server_event(AppServerEvent {
                        workspace_id: workspace_id.clone(),
                        message: json!({
                            "method": "turn/completed",
                            "params": {
                                "threadId": thread_id,
                                "turnId": turn_id,
                                "turn": { "id": turn_id, "threadId": thread_id }
                            }
                        }),
                    });
                }
                "error" => {
                    let msg = event
                        .get("error")
                        .and_then(|e| e.as_str())
                        .or_else(|| event.get("message").and_then(|m| m.as_str()))
                        .unwrap_or("Unknown Claude error");

                    turn_completed = true;

                    // Complete any partially-streamed agent messages before erroring.
                    for (msg_id, text) in &message_texts {
                        if !text.is_empty() {
                            event_sink.emit_app_server_event(AppServerEvent {
                                workspace_id: workspace_id.clone(),
                                message: json!({
                                    "method": "item/completed",
                                    "params": {
                                        "threadId": thread_id,
                                        "item": {
                                            "type": "agentMessage",
                                            "id": msg_id,
                                            "turnId": turn_id,
                                            "text": text
                                        }
                                    }
                                }),
                            });
                        }
                    }

                    event_sink.emit_app_server_event(AppServerEvent {
                        workspace_id: workspace_id.clone(),
                        message: json!({
                            "method": "error",
                            "params": {
                                "threadId": thread_id,
                                "message": msg
                            }
                        }),
                    });
                    event_sink.emit_app_server_event(AppServerEvent {
                        workspace_id: workspace_id.clone(),
                        message: json!({
                            "method": "thread/status/changed",
                            "params": {
                                "threadId": thread_id,
                                "status": { "type": "idle" }
                            }
                        }),
                    });
                    event_sink.emit_app_server_event(AppServerEvent {
                        workspace_id: workspace_id.clone(),
                        message: json!({
                            "method": "turn/completed",
                            "params": {
                                "threadId": thread_id,
                                "turnId": turn_id,
                                "turn": { "id": turn_id, "threadId": thread_id }
                            }
                        }),
                    });
                }
                _ => {}
            }
        }

        // Fallback: if Claude exited without emitting a result/error event
        // (crash, unexpected output, etc.), always mark the turn as done so
        // the UI doesn't stay stuck on "Working..." forever.
        if !turn_completed {
            eprintln!("[claude] process exited without result event — emitting fallback completion");
            // Complete any partially-streamed messages.
            for (msg_id, text) in &message_texts {
                event_sink.emit_app_server_event(AppServerEvent {
                    workspace_id: workspace_id.clone(),
                    message: json!({
                        "method": "item/completed",
                        "params": {
                            "threadId": thread_id,
                            "item": {
                                "type": "agentMessage",
                                "id": msg_id,
                                "turnId": turn_id,
                                "text": text
                            }
                        }
                    }),
                });
            }
            event_sink.emit_app_server_event(AppServerEvent {
                workspace_id: workspace_id.clone(),
                message: json!({
                    "method": "thread/status/changed",
                    "params": {
                        "threadId": thread_id,
                        "status": { "type": "idle" }
                    }
                }),
            });
            event_sink.emit_app_server_event(AppServerEvent {
                workspace_id: workspace_id.clone(),
                message: json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": thread_id,
                        "turnId": turn_id,
                        "turn": { "id": turn_id, "threadId": thread_id }
                    }
                }),
            });
        }

        if let Some(sid) = new_session_id {
            claude_state.set_session_id(&workspace_id, &thread_id, sid).await;
        }

        let _ = child.wait().await;
    });

    Ok(json!({
        "result": {
            "turn": {
                "id": turn_id_ret,
                "threadId": thread_id_ret,
            }
        }
    }))
}
