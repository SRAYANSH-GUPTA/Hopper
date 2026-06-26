use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
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
    if let Some(ref sid) = session_id {
        cmd.arg("--conversation").arg(sid);
    }
    let resolved_model = model_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("gemini-3.5-flash");
    cmd.arg("--model").arg(resolved_model);
    cmd.arg("-p").arg(&text);
    if !workspace_cwd.is_empty() {
        cmd.arg("--add-dir").arg(&workspace_cwd);
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
    let stderr = child.stderr.take().ok_or("missing stderr")?;
    
    let thread_id_clone = thread_id.clone();
    let workspace_id_clone = workspace_id.clone();
    let turn_id_clone = turn_id.clone();
    let state_clone = state.clone();
    let event_sink_clone = event_sink.clone();
    let mut current_session = session_id.clone();
    let mut current_session_clone_for_stderr = current_session.clone();

    tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("[antigravity stderr] {line}");
            if current_session_clone_for_stderr.is_none() {
                if let Some(captures) = regex::Regex::new(r"Created conversation ([a-f0-9\-]{36})").unwrap().captures(&line) {
                    let sid = captures.get(1).unwrap().as_str().to_string();
                    current_session_clone_for_stderr = Some(sid.clone());
                    state_clone.set_session_id(&workspace_id_clone, &thread_id_clone, sid.clone()).await;
                }
            }
        }
    });

    let current_session_clone = current_session.clone();
    let thread_id_t = thread_id.clone();
    let turn_id_t = turn_id.clone();
    let workspace_id_t = workspace_id.clone();
    let event_sink_t = event_sink.clone();

    tokio::spawn(async move {
        let mut sid = current_session_clone;
        for _ in 0..50 {
            if sid.is_none() {
                sid = state.get_session_id(&workspace_id_t, &thread_id_t).await;
            }
            if sid.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        if let Some(session) = sid {
            let path = dirs::home_dir()
                .unwrap()
                .join(format!(".gemini/antigravity-cli/brain/{}/.system_generated/logs/transcript.jsonl", session));
            
            let mut pos = 0;
            let mut seen_steps = std::collections::HashSet::new();
            
            // Loop for up to 2 minutes waiting for process
            for _ in 0..1200 {
                if let Ok(mut file) = std::fs::File::open(&path) {
                    use std::io::{Read, Seek, SeekFrom};
                    let _ = file.seek(SeekFrom::Start(pos));
                    let mut buf = String::new();
                    if let Ok(n) = file.read_to_string(&mut buf) {
                        if n > 0 {
                            pos += n as u64;
                            for line in buf.lines() {
                                if let Ok(val) = serde_json::from_str::<Value>(line) {
                                    let step_index = val.get("step_index").and_then(|v| v.as_i64()).unwrap_or(0);
                                    if !seen_steps.insert(step_index) { continue; }
                                    
                                    if let Some(t) = val.get("type").and_then(|v| v.as_str()) {
                                        if t == "PLANNER_RESPONSE" {
                                            if let Some(thinking) = val.get("thinking").and_then(|v| v.as_str()) {
                                                let item_id = format!("reasoning-{}", step_index);
                                                event_sink_t.emit_app_server_event(AppServerEvent {
                                                    workspace_id: workspace_id_t.clone(),
                                                    message: json!({
                                                        "method": "item/started",
                                                        "params": {
                                                            "threadId": thread_id_t,
                                                            "item": {
                                                                "type": "reasoning",
                                                                "id": item_id,
                                                                "turnId": turn_id_t,
                                                                "summary": "Thinking...",
                                                                "content": ""
                                                            }
                                                        }
                                                    }),
                                                });
                                                event_sink_t.emit_app_server_event(AppServerEvent {
                                                    workspace_id: workspace_id_t.clone(),
                                                    message: json!({
                                                        "method": "item/reasoning/textDelta",
                                                        "params": {
                                                            "threadId": thread_id_t,
                                                            "itemId": item_id,
                                                            "delta": thinking
                                                        }
                                                    }),
                                                });
                                                event_sink_t.emit_app_server_event(AppServerEvent {
                                                    workspace_id: workspace_id_t.clone(),
                                                    message: json!({
                                                        "method": "item/completed",
                                                        "params": {
                                                            "threadId": thread_id_t,
                                                            "item": {
                                                                "type": "reasoning",
                                                                "id": item_id,
                                                                "turnId": turn_id_t,
                                                                "summary": "Thinking...",
                                                                "content": thinking
                                                            }
                                                        }
                                                    }),
                                                });
                                            }
                                        } else if t != "USER_INPUT" && t != "CONVERSATION_HISTORY" && t != "EPHEMERAL_MESSAGE" && t != "CHECKPOINT" {
                                            let item_id = format!("tool-{}", step_index);
                                            let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                                            event_sink_t.emit_app_server_event(AppServerEvent {
                                                workspace_id: workspace_id_t.clone(),
                                                message: json!({
                                                    "method": "item/started",
                                                    "params": {
                                                        "threadId": thread_id_t,
                                                        "item": {
                                                            "type": "commandExecution",
                                                            "id": item_id,
                                                            "turnId": turn_id_t,
                                                            "command": t,
                                                            "status": "in_progress"
                                                        }
                                                    }
                                                }),
                                            });
                                            event_sink_t.emit_app_server_event(AppServerEvent {
                                                workspace_id: workspace_id_t.clone(),
                                                message: json!({
                                                    "method": "item/commandExecution/outputDelta",
                                                    "params": {
                                                        "threadId": thread_id_t,
                                                        "itemId": item_id,
                                                        "delta": content
                                                    }
                                                }),
                                            });
                                            event_sink_t.emit_app_server_event(AppServerEvent {
                                                workspace_id: workspace_id_t.clone(),
                                                message: json!({
                                                    "method": "item/completed",
                                                    "params": {
                                                        "threadId": thread_id_t,
                                                        "item": {
                                                            "type": "commandExecution",
                                                            "id": item_id,
                                                            "turnId": turn_id_t,
                                                            "command": t,
                                                            "status": "completed",
                                                            "output": content
                                                        }
                                                    }
                                                }),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    });

    let thread_id_ret = thread_id.clone();
    let turn_id_ret = turn_id.clone();

    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let msg_id = Uuid::new_v4().to_string();

        // Emit item/started so the UI knows a message is incoming
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

        // Stream stdout in chunks to emit live deltas
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut buffer = [0u8; 128];
        let mut full_text_buf = String::new();

        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buffer[..n]).to_string();
                    full_text_buf.push_str(&chunk);

                    event_sink.emit_app_server_event(AppServerEvent {
                        workspace_id: workspace_id.clone(),
                        message: json!({
                            "method": "item/agentMessage/delta",
                            "params": {
                                "threadId": thread_id,
                                "itemId": msg_id,
                                "delta": chunk
                            }
                        }),
                    });
                }
                Err(e) => {
                    eprintln!("[antigravity stdout stream error] {}", e);
                    break;
                }
            }
        }

        let full_text = full_text_buf.trim().to_string();
        eprintln!("[antigravity stdout complete] length: {}", full_text.len());

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
                        "text": full_text
                    }
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
