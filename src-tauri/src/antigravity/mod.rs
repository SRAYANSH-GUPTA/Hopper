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
struct AntigravityThread {
    thread_id: String,
    session_id: Option<String>,
    created_at: i64,
}

pub(crate) struct AntigravityState {
    threads: Mutex<HashMap<String, HashMap<String, AntigravityThread>>>,
}

impl AntigravityState {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            threads: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) async fn new_thread(&self, workspace_id: &str) -> String {
        let thread_id = Uuid::new_v4().to_string();
        let thread = AntigravityThread {
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

    pub(crate) async fn list_threads(&self, workspace_id: &str) -> Vec<AntigravityThread> {
        self.threads
            .lock()
            .await
            .get(workspace_id)
            .map(|map| map.values().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) async fn get_thread(&self, workspace_id: &str, thread_id: &str) -> Option<AntigravityThread> {
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
            .or_insert_with(|| AntigravityThread {
                thread_id: thread_id.to_string(),
                session_id: None,
                created_at: now_ms(),
            });
    }
}

pub(crate) async fn is_antigravity_mode(app_settings: &Mutex<crate::types::AppSettings>) -> bool {
    matches!(app_settings.lock().await.local_provider, LocalAgentProvider::Antigravity)
}

pub(crate) fn connect_workspace_antigravity<E: EventSink>(workspace_id: &str, event_sink: E) {
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.to_string(),
        message: json!({
            "method": "agent/connected",
            "params": { "workspaceId": workspace_id }
        }),
    });
}

pub(crate) async fn start_thread_antigravity<E: EventSink>(
    state: &Arc<AntigravityState>,
    workspace_id: &str,
    event_sink: E,
) -> Result<Value, String> {
    let thread_id = state.new_thread(workspace_id).await;
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

pub(crate) async fn list_threads_antigravity(
    state: &Arc<AntigravityState>,
    workspace_id: &str,
) -> Result<Value, String> {
    let threads = state.list_threads(workspace_id).await;
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

pub(crate) async fn read_thread_antigravity(
    state: &Arc<AntigravityState>,
    workspace_id: &str,
    thread_id: &str,
) -> Result<Value, String> {
    let thread = state.get_thread(workspace_id, thread_id).await;
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

async fn resolve_antigravity_bin() -> String {
    let out = Command::new("/bin/sh")
        .args(["-lc", "which agy 2>/dev/null || command -v agy 2>/dev/null"])
        .output()
        .await;

    if let Ok(o) = out {
        let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !path.is_empty() {
            return path;
        }
    }

    for candidate in &[
        "/usr/local/bin/agy",
        "/opt/homebrew/bin/agy",
    ] {
        if std::path::Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        for suffix in &[".local/bin/agy", ".npm/bin/agy"] {
            let p = format!("{home}/{suffix}");
            if std::path::Path::new(&p).exists() {
                return p;
            }
        }
    }

    "agy".to_string()
}

pub(crate) async fn send_message_antigravity<E: EventSink + 'static>(
    state: Arc<AntigravityState>,
    workspace_id: String,
    workspace_cwd: String,
    thread_id: String,
    text: String,
    model_id: Option<String>,
    event_sink: E,
) -> Result<Value, String> {
    state.ensure_thread(&workspace_id, &thread_id).await;

    let session_id = state.get_session_id(&workspace_id, &thread_id).await;
    let turn_id = Uuid::new_v4().to_string();

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

    let agy_bin = resolve_antigravity_bin().await;

    let mut cmd = Command::new(&agy_bin);
    cmd.arg("--output-format").arg("json");
    if let Some(ref sid) = session_id {
        cmd.arg("--resume").arg(sid);
    }
    let resolved_model = model_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("gemini-3.5-flash");
    cmd.arg("--model").arg(resolved_model);
    cmd.arg("-p").arg(&text);
    if !workspace_cwd.is_empty() {
        cmd.current_dir(&workspace_cwd);
    }
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "Failed to spawn antigravity (tried '{agy_bin}'). \
             Make sure the Antigravity CLI is installed: https://antigravity.google/docs/cli-using\n{e}"
        )
    })?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("[antigravity stderr] {line}");
            }
        });
    }

    let thread_id_ret = thread_id.clone();
    let turn_id_ret = turn_id.clone();

    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut new_session_id: Option<String> = None;
        let mut tool_item_ids: HashMap<String, String> = HashMap::new();
        let mut message_texts: HashMap<String, String> = HashMap::new();
        let mut started_messages: HashSet<String> = HashSet::new();
        let mut turn_completed = false;

        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            eprintln!("[antigravity stdout] {line}");

            let event: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[antigravity parse error] {e}: {line}");
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
                    let msg_obj = event.get("message");
                    let msg_id = msg_obj
                        .and_then(|m| m.get("id"))
                        .and_then(|id| id.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| Uuid::new_v4().to_string());

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
                                Some("tool_use") | Some("function_call") => {
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
                                            block.get("input")
                                                .or_else(|| block.get("args"))
                                                .cloned()
                                                .unwrap_or(json!({}));
                                        let item_id = format!("tool-{tool_use_id}");
                                        tool_item_ids.insert(tool_use_id.clone(), item_id.clone());
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

                    if !full_text.is_empty() {
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
                "tool_use" => {}
                "user" => {
                    if let Some(content_arr) = event
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        for block in content_arr {
                            let block_type = block.get("type").and_then(|t| t.as_str());
                            if block_type == Some("tool_result") || block_type == Some("function_response") {
                                let tool_use_id = block
                                    .get("tool_use_id")
                                    .or_else(|| block.get("id"))
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
                                        arr.iter()
                                            .filter_map(|b| {
                                                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                                                    b.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
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
                        .unwrap_or("Unknown Antigravity error");

                    turn_completed = true;

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

        if !turn_completed {
            eprintln!("[antigravity] process exited without result event — emitting fallback completion");
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
            state.set_session_id(&workspace_id, &thread_id, sid).await;
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
