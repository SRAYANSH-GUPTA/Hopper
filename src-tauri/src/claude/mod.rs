mod input;
pub(crate) mod permission_server;
mod stream_items;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::shared::process_core::tokio_command;
use crate::shared::claude_commands_core;
use base64::Engine;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::backend::events::{AppServerEvent, EventSink};
use crate::shared::provider_session_core::{
    ProviderEventSink, ProviderSessionStore, ProviderThread,
};
use crate::types::LocalAgentProvider;
use permission_server::ClaudePermissionServer;
use stream_items::{
    context_window, thinking_text, token_usage_payload, tool_completed_item, tool_started_item,
    TokenCounts,
};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

type ClaudeThread = ProviderThread;

pub(crate) struct ClaudeState {
    /// workspace_id -> (thread_id -> ClaudeThread)
    threads: Arc<ProviderSessionStore>,
    /// Shared permission server — started lazily on first Claude invocation.
    permission_server: Mutex<Option<Arc<ClaudePermissionServer>>>,
    /// "workspace_id:thread_id" -> child PID for interrupt support
    running_pids: crate::shared::provider_runtime_core::ProviderProcesses,
    /// "workspace_id:thread_id" -> tokens used by the thread's finished turns
    token_totals: Mutex<HashMap<String, TokenCounts>>,
}

impl ClaudeState {
    #[cfg(test)]
    pub(crate) fn new() -> Arc<Self> {
        Self::with_store(ProviderSessionStore::memory())
    }

    pub(crate) fn load(path: std::path::PathBuf) -> Result<Arc<Self>, String> {
        Ok(Self::with_store(ProviderSessionStore::load(path)?))
    }

    fn with_store(threads: Arc<ProviderSessionStore>) -> Arc<Self> {
        Arc::new(Self {
            threads,
            permission_server: Mutex::new(None),
            running_pids: crate::shared::provider_runtime_core::ProviderProcesses::default(),
            token_totals: Mutex::new(HashMap::new()),
        })
    }

    async fn token_total(&self, workspace_id: &str, thread_id: &str) -> TokenCounts {
        self.token_totals
            .lock()
            .await
            .get(&format!("{workspace_id}:{thread_id}"))
            .copied()
            .unwrap_or_default()
    }

    async fn set_token_total(&self, workspace_id: &str, thread_id: &str, total: TokenCounts) {
        self.token_totals
            .lock()
            .await
            .insert(format!("{workspace_id}:{thread_id}"), total);
    }

    async fn register_pid(
        &self,
        workspace_id: &str,
        thread_id: &str,
        pid: u32,
    ) -> Result<(), String> {
        self.running_pids
            .register(format!("{workspace_id}:{thread_id}"), pid)
            .await
    }

    async fn deregister_pid(&self, workspace_id: &str, thread_id: &str, expected_pid: Option<u32>) {
        self.running_pids
            .unregister(&format!("{workspace_id}:{thread_id}"), expected_pid)
            .await;
    }

    pub(crate) async fn kill_running_turn(&self, workspace_id: &str, thread_id: &str) {
        self.running_pids
            .interrupt(&format!("{workspace_id}:{thread_id}"))
            .await;
    }

    pub(crate) async fn shutdown(&self) {
        self.running_pids.shutdown().await;
    }

    pub(crate) async fn new_thread(&self, workspace_id: &str) -> Result<String, String> {
        let thread_id = Uuid::new_v4().to_string();
        self.threads
            .create_thread(workspace_id, &thread_id, now_ms())?;
        Ok(thread_id)
    }

    pub(crate) async fn get_session_id(
        &self,
        workspace_id: &str,
        thread_id: &str,
    ) -> Option<String> {
        self.threads.thread(workspace_id, thread_id)?.session_id
    }

    pub(crate) async fn set_session_id(
        &self,
        workspace_id: &str,
        thread_id: &str,
        session_id: String,
    ) -> Result<(), String> {
        self.threads
            .set_session_id(workspace_id, thread_id, session_id)
    }

    pub(crate) async fn list_threads(&self, workspace_id: &str) -> Vec<ClaudeThread> {
        self.threads.threads(workspace_id)
    }

    pub(crate) fn set_thread_name(
        &self,
        workspace_id: &str,
        thread_id: &str,
        name: String,
    ) -> Result<Value, String> {
        self.threads.set_name(workspace_id, thread_id, name)?;
        Ok(json!({"result": {}}))
    }

    pub(crate) async fn ensure_thread(
        &self,
        workspace_id: &str,
        thread_id: &str,
    ) -> Result<(), String> {
        self.threads.require_thread(workspace_id, thread_id)
    }

    /// Start the permission server if it isn't running yet and return its port.
    /// Hooks are supplied to each CLI session without editing global settings.
    pub(crate) async fn get_or_start_permission_server<E: EventSink>(
        &self,
        event_sink: E,
    ) -> Result<Arc<ClaudePermissionServer>, String> {
        let mut guard = self.permission_server.lock().await;
        if let Some(srv) = guard.as_ref() {
            return Ok(Arc::clone(srv));
        }
        let srv = ClaudePermissionServer::start(event_sink).await?;
        *guard = Some(Arc::clone(&srv));
        Ok(srv)
    }

    /// Resolve a pending permission request by request_id.
    /// Called from the Tauri command when the user approves or declines.
    pub(crate) async fn resolve_permission(&self, request_id: &str, approved: bool) {
        if let Some(srv) = self.permission_server.lock().await.as_ref() {
            srv.resolve(request_id, approved).await;
        }
    }

    /// Resolve a pending AskUserQuestion request with the user's answers.
    pub(crate) async fn resolve_question(&self, request_id: &str, answers: &Value) {
        if let Some(srv) = self.permission_server.lock().await.as_ref() {
            srv.resolve_question(request_id, answers).await;
        }
    }
}

/// Fetch the Claude Code model catalog and return models in the Hopper model-list format.
///
/// Primary source: `https://downloads.claude.ai/model-catalog/v1/catalog.json` (no auth).
/// Fallback: the locally cached catalog in `~/.claude/cache/model-catalog/`.
pub(crate) async fn list_models_claude() -> Result<Value, String> {
    const CATALOG_URL: &str = "https://downloads.claude.ai/model-catalog/v1/catalog.json";

    let catalog = fetch_catalog_from_url(CATALOG_URL)
        .await
        .or_else(|_| read_catalog_from_cache())?;

    let models = extract_models_from_catalog(&catalog);
    Ok(json!({ "result": { "data": models } }))
}

async fn fetch_catalog_from_url(url: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

fn read_catalog_from_cache() -> Result<Value, String> {
    let cache_dir = dirs::home_dir()
        .ok_or("Cannot locate home directory")?
        .join(".claude/cache/model-catalog");

    let entries = std::fs::read_dir(&cache_dir).map_err(|e| e.to_string())?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let mut best: Option<(i64, Value)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("published-")
            || !name.ends_with(".json")
            || name == "published-floor.json"
        {
            continue;
        }
        let bytes = match std::fs::read(entry.path()) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let wrapper: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let stale_at = wrapper.get("staleAt").and_then(Value::as_i64).unwrap_or(0);
        let fetched_at = wrapper
            .get("fetchedAt")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        // Prefer most recently fetched; accept expired catalogs as last resort.
        let doc_bytes = match wrapper.get("documentBytes").and_then(Value::as_str) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let decoded = match base64::engine::general_purpose::STANDARD.decode(&doc_bytes) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let catalog: Value = match serde_json::from_slice(&decoded) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Prefer non-expired entries; among equals pick the freshest.
        let is_fresh = stale_at > now_ms;
        let score = if is_fresh {
            fetched_at + i64::MAX / 2
        } else {
            fetched_at
        };
        if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
            best = Some((score, catalog));
        }
    }

    best.map(|(_, catalog)| catalog)
        .ok_or_else(|| "No local Claude model catalog cache found".into())
}

fn extract_models_from_catalog(catalog: &Value) -> Vec<Value> {
    let models = catalog
        .get("surfaces")
        .and_then(|s| s.get("cc"))
        .and_then(|cc| cc.get("model_selector_config"))
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|cfg| cfg.get("models"))
        .and_then(Value::as_array);

    let Some(models) = models else {
        return vec![];
    };

    models
        .iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?;
            let name = m.get("name").and_then(Value::as_str).unwrap_or(id);
            let description = m.get("description").and_then(Value::as_str).unwrap_or("");
            let runtime = m.get("runtime");
            let effort_levels: Vec<&str> = runtime
                .and_then(|r| r.get("effort_levels"))
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let default_effort = runtime
                .and_then(|r| r.get("default_effort"))
                .and_then(Value::as_str);

            let supported_reasoning_efforts: Vec<Value> = effort_levels
                .iter()
                .map(|e| json!({ "reasoningEffort": e, "description": "" }))
                .collect();

            Some(json!({
                "id": id,
                "model": id,
                "displayName": name,
                "description": description,
                "supportedReasoningEfforts": supported_reasoning_efforts,
                "defaultReasoningEffort": default_effort,
                "isDefault": false,
            }))
        })
        .collect()
}

pub(crate) async fn is_claude_mode(app_settings: &Mutex<crate::types::AppSettings>) -> bool {
    matches!(
        app_settings.lock().await.local_provider,
        LocalAgentProvider::Claude
    )
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
    let thread_id = claude_state.new_thread(workspace_id).await?;
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
                "preview": t.preview,
                "name": t.name,
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
    claude_state.threads.read_response(workspace_id, thread_id)
}

/// Resolve the absolute path of the `claude` binary by asking a login shell.
/// GUI apps on macOS/Linux don't inherit the full user PATH (nvm, homebrew,
/// npm global bins, etc.), so a bare `Command::new("claude")` silently fails.
async fn resolve_claude_bin() -> String {
    use crate::shared::provider_setup_core::{resolve_provider_bin, SetupProvider};
    resolve_provider_bin(SetupProvider::Claude)
        .await
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| SetupProvider::Claude.bin().to_string())
}

pub(crate) async fn send_message_claude<E: EventSink + 'static>(
    claude_state: Arc<ClaudeState>,
    workspace_id: String,
    workspace_cwd: String,
    thread_id: String,
    text: String,
    model_id: Option<String>,
    effort: Option<String>,
    images: Option<Vec<String>>,
    access_mode: Option<String>,
    collaboration_mode: Option<Value>,
    event_sink: E,
) -> Result<Value, String> {
    // Ensure thread exists in state
    claude_state
        .threads
        .require_resumable_for_send(&workspace_id, &thread_id)?;
    let event_sink = ProviderEventSink::new(event_sink, Arc::clone(&claude_state.threads));

    let session_id = claude_state.get_session_id(&workspace_id, &thread_id).await;
    let turn_id = Uuid::new_v4().to_string();
    let permission_mode = claude_commands_core::permission_mode(access_mode.as_deref(), collaboration_mode.as_ref())?;
    let claude_bin = resolve_claude_bin().await;
    // Unknown commands become ordinary paid model turns in newer Claude Code.
    // Query its real registry without a prompt before forwarding any slash command.
    if claude_commands_core::leading_command(&text).is_some() {
        let commands = claude_commands_core::discover_commands(&claude_bin, Some(&workspace_cwd), permission_mode).await?;
        claude_commands_core::ensure_supported(&text, &commands)?;
    }

    let image_input = images
        .as_deref()
        .filter(|images| !images.is_empty())
        .map(|images| input::image_prompt(&text, images))
        .transpose()?;

    // Build content blocks for the user message event.
    let mut content_blocks: Vec<Value> = vec![json!({ "type": "text", "text": text })];
    for image in images.as_deref().unwrap_or_default() {
        content_blocks.push(if image.starts_with("data:") {
            json!({ "type": "image", "url": image })
        } else {
            json!({ "type": "localImage", "path": image })
        });
    }

    // Start (or reuse) the permission server so hooks can route approvals to the UI.
    let permission_server = claude_state
        .get_or_start_permission_server(event_sink.clone())
        .await?;

    // Build claude command
    let mut cmd = tokio_command(&claude_bin);
    crate::shared::process_core::configure_owned_process(&mut cmd);
    cmd.arg("--output-format").arg("stream-json");
    // --verbose is required by the CLI when using --output-format=stream-json with -p.
    // In stream-json mode all output (including verbose info) is valid JSON lines.
    cmd.arg("--verbose");
    cmd.arg("--permission-mode").arg(permission_mode);
    if let Some(ref sid) = session_id {
        cmd.arg("--resume").arg(sid);
    }
    let resolved_model = model_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("claude-sonnet-4-6");
    cmd.arg("--model").arg(resolved_model);
    if let Some(effort) = effort.filter(|value| !value.trim().is_empty()) {
        cmd.arg("--effort").arg(effort);
    }
    cmd.arg("-p");
    if image_input.is_some() {
        cmd.arg("--input-format").arg("stream-json");
    } else {
        cmd.arg(&text);
    }
    if !workspace_cwd.is_empty() {
        cmd.current_dir(&workspace_cwd);
    }
    let mut hook_settings = crate::shared::provider_setup_core::claude_hook_settings(
            permission_server.port,
            &permission_server.token,
            &workspace_id,
            &thread_id,
            &turn_id,
        );
    // Bind the permission mode to this turn's authenticated callback URL. In plan
    // mode the hook must defer to Claude's policy, never approve a write itself.
    if let Some(url) = hook_settings["hooks"]["PreToolUse"][0]["hooks"][0]["url"].as_str() {
        let mut url = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
        url.query_pairs_mut().append_pair("permission_mode", permission_mode);
        hook_settings["hooks"]["PreToolUse"][0]["hooks"][0]["url"] = json!(url.as_str());
    }
    cmd.arg("--settings").arg(hook_settings.to_string());
    // Older Hopper hooks stay dormant; leave the user's global file untouched.
    cmd.env_remove("CODEXMONITOR_PERMISSION_PORT");
    cmd.env_remove("CODEXMONITOR_WORKSPACE_ID");
    cmd.stdin(if image_input.is_some() {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });
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

    // Register child PID so kill_running_turn can interrupt it.
    let child_pid = child.id();
    if let Some(pid) = child_pid {
        claude_state
            .register_pid(&workspace_id, &thread_id, pid)
            .await?;
    }

    if let Some(bytes) = image_input {
        if let Err(error) = input::write_prompt(&mut child, &bytes, Duration::from_secs(10)).await {
            claude_state
                .deregister_pid(&workspace_id, &thread_id, child_pid)
                .await;
            return Err(error);
        }
    }
    let stdout = child.stdout.take().ok_or("missing stdout")?;

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

    // Consume stderr in a background task to prevent pipe buffer deadlock.
    // Provider result/error events report failures without logging private output.
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(_)) = lines.next_line().await {}
        });
    }

    // Clone before the async move so originals are available for the return value.
    let thread_id_ret = thread_id.clone();
    let turn_id_ret = turn_id.clone();
    let usage_model = resolved_model.to_string();
    let prior_tokens = claude_state.token_total(&workspace_id, &thread_id).await;

    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        // tool_use_id -> started item (completed with the tool_result)
        let mut tool_items: HashMap<String, Value> = HashMap::new();
        // Thinking blocks already shown, keyed "message_id:block_index"
        let mut emitted_thinking: HashSet<String> = HashSet::new();
        // message_id -> latest usage for that request (repeated across snapshots)
        let mut message_usage: HashMap<String, TokenCounts> = HashMap::new();
        let mut last_usage: Option<TokenCounts> = None;
        let mut context_window_tokens = context_window(&usage_model, None);
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

            let event: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[claude parse error] {e}");
                    continue;
                }
            };

            let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match event_type {
                "system" => {
                    if let Some(text) = claude_commands_core::compact_notice(&event) {
                        let id = format!("compact-{turn_id}");
                        event_sink.emit_app_server_event(AppServerEvent {
                            workspace_id: workspace_id.clone(),
                            message: json!({"method":"item/completed","params":{"threadId":thread_id,"item":{"type":"agentMessage","id":id,"turnId":turn_id,"text":text}}}),
                        });
                    }
                    if let Some(sid) = event.get("session_id").and_then(|s| s.as_str()) {
                        if let Err(message) = claude_state
                            .set_session_id(&workspace_id, &thread_id, sid.to_string())
                            .await
                        {
                            event_sink.emit_app_server_event(AppServerEvent {
                                workspace_id: workspace_id.clone(),
                                message: json!({"method": "error", "params": {"threadId": thread_id, "error": {"message": message}, "willRetry": false}}),
                            });
                        }
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
                        for (block_index, block) in content_arr.iter().enumerate() {
                            let block_type = block.get("type").and_then(|t| t.as_str());
                            match block_type {
                                Some("thinking") => {
                                    let key = format!("{msg_id}:{block_index}");
                                    if let Some(thinking) = thinking_text(block) {
                                        if emitted_thinking.insert(key.clone()) {
                                            let item = json!({
                                                "type": "reasoning",
                                                "id": format!("reasoning-{key}"),
                                                "turnId": turn_id,
                                                "summary": "Thinking",
                                                "content": thinking,
                                            });
                                            for method in ["item/started", "item/completed"] {
                                                event_sink.emit_app_server_event(AppServerEvent {
                                                    workspace_id: workspace_id.clone(),
                                                    message: json!({
                                                        "method": method,
                                                        "params": { "threadId": thread_id, "item": item }
                                                    }),
                                                });
                                            }
                                        }
                                    }
                                }
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
                                        && !tool_items.contains_key(&tool_use_id)
                                    {
                                        let tool_name = block
                                            .get("name")
                                            .and_then(|n| n.as_str())
                                            .unwrap_or("unknown");
                                        let tool_input =
                                            block.get("input").cloned().unwrap_or(json!({}));
                                        let item = tool_started_item(
                                            &tool_use_id,
                                            tool_name,
                                            &tool_input,
                                            &turn_id,
                                        );
                                        tool_items.insert(tool_use_id.clone(), item.clone());
                                        event_sink.emit_app_server_event(AppServerEvent {
                                            workspace_id: workspace_id.clone(),
                                            message: json!({
                                                "method": "item/started",
                                                "params": { "threadId": thread_id, "item": item }
                                            }),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    // Token usage for this request; snapshots of one message repeat it.
                    if let Some(usage) = msg_obj
                        .and_then(|m| m.get("usage"))
                        .and_then(TokenCounts::from_usage)
                    {
                        message_usage.insert(msg_id.clone(), usage);
                        last_usage = Some(usage);
                        let turn_total = message_usage
                            .values()
                            .fold(TokenCounts::default(), |sum, counts| sum.add(*counts));
                        event_sink.emit_app_server_event(AppServerEvent {
                            workspace_id: workspace_id.clone(),
                            message: json!({
                                "method": "thread/tokenUsage/updated",
                                "params": {
                                    "threadId": thread_id,
                                    "tokenUsage": token_usage_payload(
                                        prior_tokens.add(turn_total),
                                        usage,
                                        context_window_tokens,
                                    ),
                                }
                            }),
                        });
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
                        if let Some(delta) = stream_items::update_text_snapshot(
                            &mut message_texts,
                            &msg_id,
                            full_text,
                        ) {
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
                                let is_error = block
                                    .get("is_error")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false);
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
                                // Complete the started item so the row keeps its tool and target.
                                let item = match tool_items.get(&tool_use_id) {
                                    Some(started) => {
                                        tool_completed_item(started, &output, is_error)
                                    }
                                    None => json!({
                                        "type": "commandExecution",
                                        "id": format!("tool-{tool_use_id}"),
                                        "turnId": turn_id,
                                        "toolUseId": tool_use_id,
                                        "status": if is_error { "failed" } else { "completed" },
                                        "aggregatedOutput": output
                                    }),
                                };
                                event_sink.emit_app_server_event(AppServerEvent {
                                    workspace_id: workspace_id.clone(),
                                    message: json!({
                                        "method": "item/completed",
                                        "params": { "threadId": thread_id, "item": item }
                                    }),
                                });
                            }
                        }
                    }
                }
                "result" => {
                    if let Some(text) = claude_commands_core::result_text(&event, message_texts.values().any(|text| !text.trim().is_empty())) {
                        message_texts.insert(format!("result-{turn_id}"), text);
                    }
                    if let Some(sid) = event.get("session_id").and_then(|s| s.as_str()) {
                        if let Err(message) = claude_state
                            .set_session_id(&workspace_id, &thread_id, sid.to_string())
                            .await
                        {
                            event_sink.emit_app_server_event(AppServerEvent {
                                workspace_id: workspace_id.clone(),
                                message: json!({"method": "error", "params": {"threadId": thread_id, "error": {"message": message}, "willRetry": false}}),
                            });
                        }
                    }
                    turn_completed = true;

                    if let Some(message) = stream_items::result_error(&event) {
                        event_sink.emit_app_server_event(AppServerEvent {
                            workspace_id: workspace_id.clone(),
                            message: json!({ "method": "error", "params": {
                                "threadId": thread_id, "turnId": turn_id,
                                "error": { "message": message }, "willRetry": false,
                            }}),
                        });
                    }

                    // The result carries the turn's authoritative usage and the real context window.
                    context_window_tokens = context_window(&usage_model, event.get("modelUsage"));
                    let turn_total = event
                        .get("usage")
                        .and_then(TokenCounts::from_usage)
                        .unwrap_or_else(|| {
                            message_usage
                                .values()
                                .fold(TokenCounts::default(), |sum, counts| sum.add(*counts))
                        });
                    let last = last_usage.unwrap_or(turn_total);
                    let thread_total = prior_tokens.add(turn_total);
                    claude_state
                        .set_token_total(&workspace_id, &thread_id, thread_total)
                        .await;
                    event_sink.emit_app_server_event(AppServerEvent {
                        workspace_id: workspace_id.clone(),
                        message: json!({
                            "method": "thread/tokenUsage/updated",
                            "params": {
                                "threadId": thread_id,
                                "tokenUsage": token_usage_payload(thread_total, last, context_window_tokens),
                            }
                        }),
                    });

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
            event_sink.emit_app_server_event(AppServerEvent {
                workspace_id: workspace_id.clone(),
                message: json!({"method": "error", "params": {"threadId": thread_id, "turnId": turn_id,
                    "error": {"message": "Claude exited without completing the turn. Check your provider sign-in and try again."}, "willRetry": false}}),
            });
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

        let _ = child.wait().await;
        claude_state
            .deregister_pid(&workspace_id, &thread_id, child_pid)
            .await;
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
    fn saved_threads_resume_with_the_same_provider_session_and_name() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let dir = std::env::temp_dir().join(format!("hopper-claude-{}", Uuid::new_v4()));
                let path = dir.join("conversations.json");
                let state = ClaudeState::load(path.clone()).unwrap();
                let thread = state.new_thread("workspace").await.unwrap();
                state
                    .set_session_id("workspace", &thread, "provider-session".into())
                    .await
                    .unwrap();
                state
                    .set_thread_name("workspace", &thread, "Saved conversation".into())
                    .unwrap();
                drop(state);
                let state = ClaudeState::load(path).unwrap();
                assert_eq!(
                    state.get_session_id("workspace", &thread).await.as_deref(),
                    Some("provider-session")
                );
                assert!(state.ensure_thread("workspace", "missing").await.is_err());
                assert!(state
                    .ensure_thread("other-workspace", &thread)
                    .await
                    .is_err());
                assert_eq!(
                    read_thread_claude(&state, "workspace", &thread)
                        .await
                        .unwrap()["result"]["thread"]["name"],
                    "Saved conversation"
                );
                assert_eq!(
                    list_threads_claude(&state, "workspace").await.unwrap()["result"]["data"][0]
                        ["id"],
                    thread
                );
                drop(state);
                std::fs::remove_dir_all(dir).unwrap();
            });
    }
}
