use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

use crate::backend::events::{AppServerEvent, EventSink};

/// Lightweight HTTP server that bridges Claude Code's PreToolUse hooks to the
/// CodexMonitor frontend approval UI.
///
/// Flow:
///   Claude Code (hook) → POST /permission → server holds connection →
///   emits claude/requestApproval event → frontend shows toast →
///   user approves/declines → Tauri command resolves oneshot →
///   server writes {"decision":"approve"} or {"decision":"block"} →
///   hook exits → Claude Code proceeds or stops.
pub(crate) struct ClaudePermissionServer {
    pub(crate) port: u16,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

impl ClaudePermissionServer {
    pub(crate) async fn start<E: EventSink>(event_sink: E) -> Arc<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind Claude permission server");
        let port = listener.local_addr().unwrap().port();
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = pending.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let pending = pending_clone.clone();
                        let sink = event_sink.clone();
                        tokio::spawn(handle_connection(stream, pending, sink));
                    }
                    Err(e) => {
                        eprintln!("[claude permission server] accept error: {e}");
                    }
                }
            }
        });

        eprintln!("[claude permission server] started on port {port}");
        Arc::new(Self { port, pending })
    }

    /// Called by the Tauri command when the user approves or declines.
    pub(crate) async fn resolve(&self, request_id: &str, approved: bool) {
        let mut guard = self.pending.lock().await;
        if let Some(tx) = guard.remove(request_id) {
            let _ = tx.send(approved);
        }
    }
}

async fn handle_connection<E: EventSink>(
    mut stream: TcpStream,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    event_sink: E,
) {
    // Read the full HTTP request (hooks send small payloads, 64 KiB is plenty)
    let mut buf = vec![0u8; 65536];
    let n = match stream.read(&mut buf).await {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let req = String::from_utf8_lossy(&buf[..n]);

    // Parse workspace_id from the query string: GET /permission?workspace_id=...
    let workspace_id = req
        .lines()
        .next()                          // "POST /permission?workspace_id=xxx HTTP/1.1"
        .and_then(|line| {
            let path = line.split_whitespace().nth(1)?;
            let query = path.split_once('?')?.1;
            query.split('&').find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                if k == "workspace_id" { Some(v.to_string()) } else { None }
            })
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Extract JSON body (everything after the blank line separating headers from body)
    let body = match req.find("\r\n\r\n") {
        Some(pos) => req[pos + 4..].trim().to_string(),
        None => return,
    };

    if body.is_empty() {
        let _ = write_http_response(&mut stream, 400, r#"{"error":"empty body"}"#).await;
        return;
    }

    // Claude Code PreToolUse hook sends:
    // {"tool_name":"Bash","tool_input":{"command":"..."}}
    let data: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[claude permission server] JSON parse error: {e}: {body}");
            let _ = write_http_response(&mut stream, 400, r#"{"error":"invalid json"}"#).await;
            return;
        }
    };

    let request_id = format!("claude-{}", Uuid::new_v4());

    let tool_name = data
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let tool_input = data.get("tool_input").cloned().unwrap_or(json!({}));

    // Flatten tool_input fields into params so the existing ApprovalToasts renders them
    let mut params = serde_json::Map::new();
    params.insert("tool".to_string(), json!(tool_name));
    if let Some(obj) = tool_input.as_object() {
        for (k, v) in obj {
            params.insert(k.clone(), v.clone());
        }
    }

    // Register the oneshot channel before emitting the event so the UI can
    // immediately call resolve() without a race.
    let (tx, rx) = oneshot::channel::<bool>();
    pending.lock().await.insert(request_id.clone(), tx);

    // Emit the approval event. The frontend's isApprovalRequestMethod() will
    // detect it because the method ends with "requestApproval".
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.clone(),
        message: json!({
            "method": "claude/requestApproval",
            "id": request_id,
            "params": Value::Object(params)
        }),
    });

    // Hold the HTTP connection open while waiting for the user's decision.
    // Timeout after 120 seconds; default to block on timeout for safety.
    let approved = tokio::time::timeout(std::time::Duration::from_secs(120), rx)
        .await
        .unwrap_or(Ok(false))
        .unwrap_or(false);

    // Remove the pending entry in case it wasn't consumed (e.g. timeout path)
    pending.lock().await.remove(&request_id);

    let response_body = if approved {
        r#"{"decision":"approve"}"#
    } else {
        r#"{"decision":"block","reason":"User declined in CodexMonitor"}"#
    };

    let _ = write_http_response(&mut stream, 200, response_body).await;
}

async fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    body: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await
}

/// Write the CodexMonitor PreToolUse hook into ~/.claude/settings.json so that
/// all Claude Code invocations (in any workspace) can route permission requests
/// through CodexMonitor when it is running.
///
/// The hook is a no-op when CODEXMONITOR_PERMISSION_PORT is unset (i.e. when
/// Claude Code is run outside of CodexMonitor).
pub(crate) async fn ensure_hook_installed() {
    let hook_command = concat!(
        "if [ -n \"$CODEXMONITOR_PERMISSION_PORT\" ]; then ",
        "INPUT=$(cat); ",
        "PAYLOAD=$(printf '%s' \"$INPUT\" | python3 -c \"",
        "import sys,json; d=json.load(sys.stdin); ",
        "d['workspace_id']=__import__('os').environ.get('CODEXMONITOR_WORKSPACE_ID','unknown'); ",
        "print(json.dumps(d))\"); ",
        "curl -s --max-time 120 -X POST -H 'Content-Type: application/json' ",
        "-d \"$PAYLOAD\" ",
        "\"http://127.0.0.1:$CODEXMONITOR_PERMISSION_PORT/permission\" 2>/dev/null || true; ",
        "fi"
    );

    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return,
    };
    let settings_path = std::path::PathBuf::from(&home).join(".claude").join("settings.json");

    // Read existing settings (or start with empty object)
    let raw = tokio::fs::read_to_string(&settings_path).await.unwrap_or_default();
    let mut settings: Value = serde_json::from_str(&raw).unwrap_or(json!({}));

    // Navigate to hooks.PreToolUse array, creating it if absent
    let hooks = settings
        .as_object_mut()
        .and_then(|o| {
            if !o.contains_key("hooks") {
                o.insert("hooks".to_string(), json!({}));
            }
            o.get_mut("hooks")
        })
        .and_then(|h| h.as_object_mut())
        .and_then(|h| {
            if !h.contains_key("PreToolUse") {
                h.insert("PreToolUse".to_string(), json!([]));
            }
            h.get_mut("PreToolUse")
        })
        .and_then(|ptu| ptu.as_array_mut());

    let hooks = match hooks {
        Some(h) => h,
        None => return,
    };

    // Check if our hook is already present (avoid duplicates)
    let already_installed = hooks.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|arr| {
                arr.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .map(|c| c.contains("CODEXMONITOR_PERMISSION_PORT"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    });

    if already_installed {
        return;
    }

    // Insert the hook entry that catches all tools (".*" matcher)
    hooks.push(json!({
        "matcher": ".*",
        "hooks": [{
            "type": "command",
            "command": hook_command
        }]
    }));

    // Write back
    if let Ok(serialized) = serde_json::to_string_pretty(&settings) {
        let _ = tokio::fs::write(&settings_path, serialized).await;
        eprintln!("[claude permission server] hook installed in ~/.claude/settings.json");
    }
}
