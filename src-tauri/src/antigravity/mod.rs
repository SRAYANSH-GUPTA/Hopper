use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde_json::{json, Value};
use crate::shared::process_core::{terminate_process, tokio_command};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::backend::events::{AppServerEvent, EventSink};
use crate::types::LocalAgentProvider;

mod steps;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Transcript polling runs every 100 ms; stop after 6 hours even if agy never exits.
const TRANSCRIPT_MAX_POLLS: usize = 6 * 60 * 60 * 10;
/// Keep reading for ~1 s after stdout ends so the final transcript steps arrive.
const TRANSCRIPT_POLLS_AFTER_FINISH: usize = 10;

/// Prefix of request ids Hopper issues for agy (questions and permission retries).
pub(crate) const ANTIGRAVITY_REQUEST_PREFIX: &str = "agy-";

/// The questions of an `ask_question` tool call in a PLANNER_RESPONSE step.
fn ask_question_calls(step: &Value) -> Option<Vec<Value>> {
    let questions: Vec<Value> = step
        .get("tool_calls")?
        .as_array()?
        .iter()
        .filter(|call| call.get("name").and_then(Value::as_str) == Some("ask_question"))
        .filter_map(|call| call.pointer("/args/questions").and_then(Value::as_array))
        .flatten()
        .cloned()
        .collect();
    (!questions.is_empty()).then_some(questions)
}

/// Maps agy questions to question-card params. agy can't wait for answers in
/// headless mode, so they are answered with a follow-up message.
fn question_request_params(
    questions: &[Value],
    thread_id: &str,
    turn_id: &str,
    item_id: &str,
) -> Value {
    let mapped: Vec<Value> = questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let options: Vec<Value> = question
                .get("options")
                .and_then(Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .filter_map(|option| match option {
                            Value::String(label) => Some(label.clone()),
                            _ => option.get("label").and_then(Value::as_str).map(str::to_string),
                        })
                        .map(|label| json!({ "label": label, "description": "" }))
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "id": format!("q{index}"),
                "header": "",
                "question": question.get("question").and_then(Value::as_str).unwrap_or(""),
                "options": options,
                "multiSelect": question.get("is_multi_select").and_then(Value::as_bool).unwrap_or(false),
                "isOther": true,
            })
        })
        .collect();
    json!({
        "threadId": thread_id,
        "turnId": turn_id,
        "itemId": item_id,
        "answerMode": "followUp",
        "questions": mapped,
    })
}

/// The permission named in agy's headless auto-deny message, if `line` is one.
fn denied_permission(line: &str) -> Option<String> {
    if !line.contains("auto-denied") {
        return None;
    }
    regex::Regex::new(r#"required the "([^"]+)" permission"#)
        .ok()?
        .captures(line)
        .map(|captures| captures[1].to_string())
}

#[derive(Clone)]
struct AntigravityThread {
    thread_id: String,
    session_id: Option<String>,
    created_at: i64,
}

/// What a turn was started with, kept so a permission-blocked turn can be rerun.
#[derive(Clone, Debug, PartialEq)]
struct TurnInput {
    workspace_cwd: String,
    text: String,
    model_id: Option<String>,
    images: Option<Vec<String>>,
}

/// Per-turn overrides.
#[derive(Clone, Copy, Debug, Default)]
struct TurnOptions {
    /// Run this turn with `--dangerously-skip-permissions` regardless of the setting.
    allow_all_permissions: bool,
    /// Rerun of an earlier turn: don't show the user's message again.
    replay: bool,
}

pub(crate) struct AntigravityState {
    threads: Mutex<HashMap<String, HashMap<String, AntigravityThread>>>,
    /// "workspace_id:thread_id" -> child PID for interrupt support
    running_pids: Mutex<HashMap<String, u32>>,
    /// "workspace_id:thread_id" -> input of the thread's latest turn
    last_turns: Mutex<HashMap<String, TurnInput>>,
    /// Permission retry request id -> (workspace_id, thread_id)
    pending_retries: Mutex<HashMap<String, (String, String)>>,
}

impl AntigravityState {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            threads: Mutex::new(HashMap::new()),
            running_pids: Mutex::new(HashMap::new()),
            last_turns: Mutex::new(HashMap::new()),
            pending_retries: Mutex::new(HashMap::new()),
        })
    }

    async fn remember_turn(&self, workspace_id: &str, thread_id: &str, input: TurnInput) {
        self.last_turns
            .lock()
            .await
            .insert(format!("{workspace_id}:{thread_id}"), input);
    }

    async fn register_retry(&self, workspace_id: &str, thread_id: &str) -> String {
        let request_id = format!("{ANTIGRAVITY_REQUEST_PREFIX}perm-{}", Uuid::new_v4());
        self.pending_retries.lock().await.insert(
            request_id.clone(),
            (workspace_id.to_string(), thread_id.to_string()),
        );
        request_id
    }

    async fn take_retry(&self, request_id: &str) -> Option<(String, String, TurnInput)> {
        let (workspace_id, thread_id) = self.pending_retries.lock().await.remove(request_id)?;
        let input = self
            .last_turns
            .lock()
            .await
            .get(&format!("{workspace_id}:{thread_id}"))
            .cloned()?;
        Some((workspace_id, thread_id, input))
    }

    async fn register_pid(&self, workspace_id: &str, thread_id: &str, pid: u32) {
        let key = format!("{workspace_id}:{thread_id}");
        self.running_pids.lock().await.insert(key, pid);
    }

    async fn deregister_pid(&self, workspace_id: &str, thread_id: &str) {
        let key = format!("{workspace_id}:{thread_id}");
        self.running_pids.lock().await.remove(&key);
    }

    pub(crate) async fn kill_running_turn(&self, workspace_id: &str, thread_id: &str) {
        let key = format!("{workspace_id}:{thread_id}");
        let pid = self.running_pids.lock().await.remove(&key);
        if let Some(pid) = pid {
            terminate_process(pid).await;
        }
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
    use crate::shared::provider_setup_core::{resolve_provider_bin, SetupProvider};
    resolve_provider_bin(SetupProvider::Antigravity).await
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| SetupProvider::Antigravity.bin().to_string())
}

fn data_url_to_temp_file(data_url: &str) -> Option<String> {
    let comma = data_url.find(',')?;
    let header = &data_url[..comma];
    let data = &data_url[comma + 1..];
    let ext = if header.contains("image/png") {
        "png"
    } else if header.contains("image/jpeg") || header.contains("image/jpg") {
        "jpg"
    } else if header.contains("image/gif") {
        "gif"
    } else if header.contains("image/webp") {
        "webp"
    } else {
        "png"
    };
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
    let path = std::env::temp_dir().join(format!("hopper-img-{}.{}", Uuid::new_v4(), ext));
    std::fs::write(&path, bytes).ok()?;
    Some(path.to_string_lossy().into_owned())
}

pub(crate) async fn send_message_antigravity<E: EventSink + 'static>(
    state: Arc<AntigravityState>,
    workspace_id: String,
    workspace_cwd: String,
    thread_id: String,
    text: String,
    model_id: Option<String>,
    images: Option<Vec<String>>,
    event_sink: E,
) -> Result<Value, String> {
    let input = TurnInput {
        workspace_cwd,
        text,
        model_id,
        images,
    };
    run_turn(state, workspace_id, thread_id, input, TurnOptions::default(), event_sink).await
}

/// Answers a permission-retry prompt: on accept, reruns the blocked turn with
/// all permissions allowed for that run only.
pub(crate) async fn respond_to_permission_retry<E: EventSink + 'static>(
    state: Arc<AntigravityState>,
    request_id: &str,
    accept: bool,
    event_sink: E,
) -> Result<(), String> {
    let Some((workspace_id, thread_id, input)) = state.take_retry(request_id).await else {
        return Ok(());
    };
    if !accept {
        return Ok(());
    }
    let options = TurnOptions {
        allow_all_permissions: true,
        replay: true,
    };
    run_turn(state, workspace_id, thread_id, input, options, event_sink)
        .await
        .map(|_| ())
}

async fn run_turn<E: EventSink + 'static>(
    state: Arc<AntigravityState>,
    workspace_id: String,
    thread_id: String,
    input: TurnInput,
    options: TurnOptions,
    event_sink: E,
) -> Result<Value, String> {
    let auto_approve = options.allow_all_permissions
        || crate::shared::provider_setup_core::read_preferences()?.antigravity_auto_approve;
    state.ensure_thread(&workspace_id, &thread_id).await;
    state.remember_turn(&workspace_id, &thread_id, input.clone()).await;
    let TurnInput {
        workspace_cwd,
        text,
        model_id,
        images,
    } = input;

    let session_id = state.get_session_id(&workspace_id, &thread_id).await;
    let turn_id = Uuid::new_v4().to_string();

    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.clone(),
        message: json!({
            "method": "thread/status/changed",
            "params": {
                "threadId": thread_id,
                "status": { "type": "active" }
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

    let mut final_text = text.clone();
    let mut content_blocks = vec![json!({ "type": "text", "text": text })];
    let mut temp_files: Vec<String> = Vec::new();

    if let Some(ref image_list) = images {
        for image in image_list {
            let file_path = if image.starts_with("data:") {
                match data_url_to_temp_file(image) {
                    Some(p) => {
                        temp_files.push(p.clone());
                        p
                    }
                    None => continue,
                }
            } else {
                image.clone()
            };
            final_text.push_str(&format!(" @{}", file_path));
            content_blocks.push(json!({ "type": "localImage", "path": file_path }));
        }
    }

    if !options.replay {
        let user_item_id = Uuid::new_v4().to_string();
        let user_item = json!({
            "type": "userMessage",
            "id": user_item_id,
            "turnId": turn_id,
            "content": content_blocks
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
    }

    let agy_bin = resolve_antigravity_bin().await;

    let mut cmd = tokio_command(&agy_bin);
    if let Some(ref sid) = session_id {
        cmd.arg("--conversation").arg(sid);
    }
    let resolved_model = model_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Gemini 3.8 Flash (Medium)");
    cmd.arg("--model").arg(resolved_model);
    if auto_approve {
        cmd.arg("--dangerously-skip-permissions");
    }
    cmd.arg("-p").arg(&final_text);
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

    // Register child PID so kill_running_turn can interrupt it.
    if let Some(pid) = child.id() {
        state.register_pid(&workspace_id, &thread_id, pid).await;
    }

    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let stderr = child.stderr.take().ok_or("missing stderr")?;
    
    let thread_id_clone = thread_id.clone();
    let workspace_id_clone = workspace_id.clone();
    let state_clone = state.clone();
    let state_for_deregister = state.clone();
    let event_sink_clone = event_sink.clone();
    let current_session = session_id.clone();
    let mut current_session_clone_for_stderr = current_session.clone();
    let turn_id_for_stderr = turn_id.clone();
    let workspace_id_for_deregister = workspace_id.clone();
    let thread_id_for_deregister = thread_id.clone();

    tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        let mut emitted_error = false;
        let mut prompted_permission = false;
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("[antigravity stderr] {line}");
            if let Some(permission) = denied_permission(&line).filter(|_| !prompted_permission) {
                prompted_permission = true;
                let message = format!(
                    "agy needs the \"{permission}\" permission, which Hopper isn't granting automatically. Allow it to retry this turn."
                );
                event_sink_clone.emit_app_server_event(AppServerEvent {
                    workspace_id: workspace_id_clone.clone(),
                    message: json!({
                        "method": "error",
                        "params": {
                            "threadId": thread_id_clone.clone(),
                            "turnId": turn_id_for_stderr.clone(),
                            "error": { "message": message },
                            "willRetry": false
                        }
                    }),
                });
                let request_id = state_clone
                    .register_retry(&workspace_id_clone, &thread_id_clone)
                    .await;
                event_sink_clone.emit_app_server_event(AppServerEvent {
                    workspace_id: workspace_id_clone.clone(),
                    message: json!({
                        "method": "antigravity/requestApproval",
                        "id": request_id,
                        "params": {
                            "tool": permission,
                            "threadId": thread_id_clone.clone(),
                            "turnId": turn_id_for_stderr.clone(),
                            "reason": message,
                        }
                    }),
                });
                continue;
            }
            let lower = line.to_ascii_lowercase();
            if !emitted_error && (lower.contains("quota") || lower.contains("rate limit")) {
                emitted_error = true;
                event_sink_clone.emit_app_server_event(AppServerEvent {
                    workspace_id: workspace_id_clone.clone(),
                    message: json!({
                        "method": "error",
                        "params": {
                            "threadId": thread_id_clone.clone(),
                            "turnId": turn_id_for_stderr.clone(),
                            "error": { "message": line },
                            "willRetry": false
                        }
                    }),
                });
            }
            if current_session_clone_for_stderr.is_none() {
                if let Some(captures) = regex::Regex::new(r"Created conversation ([a-f0-9\-]{36})").unwrap().captures(&line) {
                    let sid = captures.get(1).unwrap().as_str().to_string();
                    current_session_clone_for_stderr = Some(sid.clone());
                    state_clone.set_session_id(&workspace_id_clone, &thread_id_clone, sid.clone()).await;
                }
            }
        }
    });

    // Set once agy's stdout ends, so the transcript tailer knows to finish up.
    let turn_finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let turn_finished_t = turn_finished.clone();
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
            let mut pending_questions: Option<Vec<Value>> = None;
            let mut pending_calls = steps::PendingToolCalls::default();
            let mut polls_after_finish = 0;

            // Follow the transcript until the turn ends (bounded for safety).
            for _ in 0..TRANSCRIPT_MAX_POLLS {
                let finished = turn_finished_t.load(std::sync::atomic::Ordering::SeqCst);
                if let Ok(mut file) = std::fs::File::open(&path) {
                    use std::io::{Read, Seek, SeekFrom};
                    let _ = file.seek(SeekFrom::Start(pos));
                    let mut buf = String::new();
                    if file.read_to_string(&mut buf).is_ok() {
                        // Consume complete lines only; a line still being written is re-read next poll.
                        let complete = buf.rfind('\n').map(|index| index + 1).unwrap_or(0);
                        if complete > 0 {
                            pos += complete as u64;
                            for line in buf[..complete].lines() {
                                if let Ok(val) = serde_json::from_str::<Value>(line) {
                                    let step_index = val.get("step_index").and_then(|v| v.as_i64()).unwrap_or(0);
                                    if !seen_steps.insert(step_index) { continue; }
                                    
                                    if let Some(t) = val.get("type").and_then(|v| v.as_str()) {
                                        if t == "PLANNER_RESPONSE" {
                                            if let Some(questions) = ask_question_calls(&val) {
                                                pending_questions = Some(questions);
                                            }
                                            pending_calls.extend_from_planner(&val);
                                            if let Some(thinking) = val
                                                .get("thinking")
                                                .and_then(|v| v.as_str())
                                                .filter(|thinking| !thinking.trim().is_empty())
                                            {
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
                                        } else if !matches!(t, "USER_INPUT" | "CONVERSATION_HISTORY" | "EPHEMERAL_MESSAGE" | "CHECKPOINT" | "SYSTEM_MESSAGE") {
                                            let item_id = format!("tool-{}", step_index);
                                            let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                                            let asked = if t == "ASK_QUESTION" { pending_questions.take() } else { None };
                                            let (started, completed) = match asked.as_ref() {
                                                Some(questions) => {
                                                    let item = json!({
                                                        "type": "toolCall",
                                                        "id": item_id,
                                                        "turnId": turn_id_t,
                                                        "tool": "ask_question",
                                                        "title": "Asked",
                                                        "detail": questions
                                                            .first()
                                                            .and_then(|question| question.get("question").and_then(Value::as_str))
                                                            .unwrap_or(""),
                                                        "status": "completed",
                                                        "output": "agy can't wait for answers in headless mode. Answer below to send your reply.",
                                                    });
                                                    (item.clone(), item)
                                                }
                                                None => {
                                                    let call = pending_calls.take_for_step(t);
                                                    let failed = t == "ERROR_MESSAGE"
                                                        || val.get("error").is_some_and(|error| !error.is_null());
                                                    steps::step_items(&item_id, &turn_id_t, t, call.as_ref(), content, failed)
                                                }
                                            };
                                            for (method, item) in [("item/started", started), ("item/completed", completed)] {
                                                event_sink_t.emit_app_server_event(AppServerEvent {
                                                    workspace_id: workspace_id_t.clone(),
                                                    message: json!({
                                                        "method": method,
                                                        "params": { "threadId": thread_id_t, "item": item }
                                                    }),
                                                });
                                            }
                                            if let Some(questions) = asked {
                                                event_sink_t.emit_app_server_event(AppServerEvent {
                                                    workspace_id: workspace_id_t.clone(),
                                                    message: json!({
                                                        "method": "item/tool/requestUserInput",
                                                        "id": format!("{ANTIGRAVITY_REQUEST_PREFIX}q-{}", Uuid::new_v4()),
                                                        "params": question_request_params(&questions, &thread_id_t, &turn_id_t, &item_id),
                                                    }),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if finished {
                    polls_after_finish += 1;
                    if polls_after_finish >= TRANSCRIPT_POLLS_AFTER_FINISH {
                        break;
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

        turn_finished.store(true, std::sync::atomic::Ordering::SeqCst);
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
        state_for_deregister.deregister_pid(&workspace_id_for_deregister, &thread_id_for_deregister).await;
        for tmp in &temp_files {
            let _ = std::fs::remove_file(tmp);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ask_question_calls_from_planner_steps() {
        let step = json!({
            "type": "PLANNER_RESPONSE",
            "tool_calls": [
                {"name": "view_file", "args": {"path": "a"}},
                {"name": "ask_question", "args": {"questions": [
                    {"question": "Which part of the UI?", "options": ["Home", "Sidebar"], "is_multi_select": true}
                ]}}
            ]
        });
        let questions = ask_question_calls(&step).unwrap();
        assert_eq!(questions.len(), 1);
        assert!(ask_question_calls(&json!({"tool_calls": []})).is_none());
        assert!(ask_question_calls(&json!({})).is_none());

        let params = question_request_params(&questions, "thread-1", "turn-1", "tool-74");
        assert_eq!(params["answerMode"], "followUp");
        assert_eq!(params["itemId"], "tool-74");
        assert_eq!(params["questions"][0]["id"], "q0");
        assert_eq!(params["questions"][0]["question"], "Which part of the UI?");
        assert_eq!(params["questions"][0]["multiSelect"], true);
        assert_eq!(params["questions"][0]["options"][1]["label"], "Sidebar");
    }

    #[test]
    fn detects_headless_permission_denials() {
        let line = "jetski: no output produced — a tool required the \"command\" permission that headless mode cannot prompt for, so it was auto-denied. Add an allow-rule under permissions.allow in settings.json.";
        assert_eq!(denied_permission(line).as_deref(), Some("command"));
        assert!(denied_permission("Created conversation 1234").is_none());
        assert!(denied_permission("required the \"command\" permission").is_none());
    }

    #[test]
    fn retries_only_registered_requests_once() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let state = AntigravityState::new();
                let input = TurnInput {
                    workspace_cwd: "/tmp".into(),
                    text: "run ls".into(),
                    model_id: None,
                    images: None,
                };
                state.remember_turn("ws", "thread", input.clone()).await;
                let request_id = state.register_retry("ws", "thread").await;
                assert!(request_id.starts_with(ANTIGRAVITY_REQUEST_PREFIX));
                assert_eq!(
                    state.take_retry(&request_id).await,
                    Some(("ws".into(), "thread".into(), input))
                );
                assert!(state.take_retry(&request_id).await.is_none());
                assert!(state.take_retry("agy-perm-unknown").await.is_none());
            });
    }
}
