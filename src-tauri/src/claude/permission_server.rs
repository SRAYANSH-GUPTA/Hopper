use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

use crate::backend::events::{AppServerEvent, EventSink};

/// Lightweight HTTP server that bridges Claude Code's PreToolUse hooks to the
/// Hopper frontend approval UI.
///
/// Flow:
///   Claude Code (hook) → POST /permission → server holds connection →
///   emits claude/requestApproval event → frontend shows toast →
///   user approves/declines → Tauri command resolves oneshot →
///   server returns PreToolUse permissionDecision allow/deny →
///   hook exits → Claude Code proceeds or stops.
pub(crate) struct ClaudePermissionServer {
    pub(crate) port: u16,
    pub(crate) token: String,
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
        let token = Uuid::new_v4().to_string();
        let server_token = token.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let pending = pending_clone.clone();
                        let sink = event_sink.clone();
                        tokio::spawn(handle_connection(
                            stream,
                            pending,
                            sink,
                            server_token.clone(),
                        ));
                    }
                    Err(e) => {
                        eprintln!("[claude permission server] accept error: {e}");
                    }
                }
            }
        });

        eprintln!("[claude permission server] started on port {port}");
        Arc::new(Self {
            port,
            token,
            pending,
        })
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
    token: String,
) {
    let request = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        read_request(&mut stream),
    )
    .await;
    let (headers, body) = match request {
        Ok(Ok(request)) => request,
        _ => {
            let _ = write_http_response(&mut stream, 400, r#"{"error":"invalid request"}"#).await;
            return;
        }
    };
    let authorized = headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, value)| {
            key.eq_ignore_ascii_case("x-hopper-token") && value.trim() == token
        })
    });
    if !authorized {
        let _ = write_http_response(&mut stream, 403, r#"{"error":"forbidden"}"#).await;
        return;
    }
    let workspace_id = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| reqwest::Url::parse(&format!("http://localhost{path}")).ok())
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "workspace_id")
                .map(|(_, value)| value.into_owned())
        });
    let Some(workspace_id) = workspace_id else {
        let _ = write_http_response(&mut stream, 400, r#"{"error":"missing workspace"}"#).await;
        return;
    };

    // Claude Code PreToolUse hook sends:
    // {"tool_name":"Bash","tool_input":{"command":"..."}}
    let data: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[claude permission server] JSON parse error: {e}");
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

    let response_body =
        crate::shared::provider_setup_core::claude_permission_response(approved).to_string();
    let _ = write_http_response(&mut stream, 200, &response_body).await;
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

async fn read_request(stream: &mut TcpStream) -> Result<(String, String), String> {
    const LIMIT: usize = 1024 * 1024;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("Incomplete HTTP request".into());
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > LIMIT {
            return Err("Request too large".into());
        }
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..end]).map_err(|e| e.to_string())?;
            if !headers.starts_with("POST /permission?") {
                return Err("Unknown route".into());
            }
            let size = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    if key.eq_ignore_ascii_case("content-length") {
                        value.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .ok_or("Missing content length")?;
            if size > LIMIT - end - 4 {
                return Err("Body too large".into());
            }
            if bytes.len() >= end + 4 + size {
                let body = std::str::from_utf8(&bytes[end + 4..end + 4 + size])
                    .map_err(|e| e.to_string())?;
                return Ok((headers.to_string(), body.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestSink(tokio::sync::mpsc::UnboundedSender<AppServerEvent>);
    impl EventSink for TestSink {
        fn emit_app_server_event(&self, event: AppServerEvent) {
            let _ = self.0.send(event);
        }
        fn emit_terminal_output(&self, _: crate::backend::events::TerminalOutput) {}
        fn emit_terminal_exit(&self, _: crate::backend::events::TerminalExit) {}
    }

    #[test]
    fn authenticates_and_routes_claude_approval_requests() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                let server = ClaudePermissionServer::start(TestSink(tx)).await;
                let settings = crate::shared::provider_setup_core::claude_hook_settings(
                    server.port,
                    &server.token,
                    "workspace & one",
                );
                let url = settings["hooks"]["PreToolUse"][0]["hooks"][0]["url"]
                    .as_str()
                    .unwrap()
                    .to_string();
                let client = reqwest::Client::new();
                let rejected = client
                    .post(&url)
                    .json(&json!({"tool_name": "Bash"}))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(rejected.status(), 403);
                assert!(rx.try_recv().is_err());
                let token = server.token.clone();
                let response = tokio::spawn(async move {
                    client
                        .post(&url)
                        .header("X-Hopper-Token", token)
                        .json(&json!({"tool_name": "Bash", "tool_input": {"command": "pwd"}}))
                        .send()
                        .await
                        .unwrap()
                        .json::<Value>()
                        .await
                        .unwrap()
                });
                let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(event.workspace_id, "workspace & one");
                server
                    .resolve(event.message["id"].as_str().unwrap(), true)
                    .await;
                assert_eq!(
                    response.await.unwrap()["hookSpecificOutput"]["permissionDecision"],
                    "allow"
                );
                assert!(server.pending.lock().await.is_empty());
            });
    }

    #[test]
    fn reads_a_body_split_across_tcp_packets() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let sender = tokio::spawn(async move {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            stream.write_all(b"POST /permission?workspace_id=one HTTP/1.1\r\nContent-Length: 7\r\n\r\n").await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            stream.write_all(b"{\"x\":1}").await.unwrap();
        });
        let (mut stream, _) = listener.accept().await.unwrap();
        let (_, body) = read_request(&mut stream).await.unwrap();
        assert_eq!(body, r#"{"x":1}"#);
        sender.await.unwrap();
        });
    }
}
