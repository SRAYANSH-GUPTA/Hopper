use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

use crate::backend::events::{AppServerEvent, EventSink};
use crate::shared::provider_setup_core::{
    claude_permission_response, ClaudeHookOutcome, CLAUDE_HOOK_TIMEOUT_SECS,
};

const APPROVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
/// Leaves headroom under the hook timeout so Hopper always answers first.
const QUESTION_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(CLAUDE_HOOK_TIMEOUT_SECS - 20);
const ASK_USER_QUESTION_TOOL: &str = "AskUserQuestion";

struct PendingRequest {
    sender: oneshot::Sender<ClaudeHookOutcome>,
    /// Original AskUserQuestion input, present for question requests.
    questions_input: Option<Value>,
}

type PendingMap = Arc<Mutex<HashMap<String, PendingRequest>>>;

/// Lightweight HTTP server that bridges Claude Code's PreToolUse hooks to the
/// Hopper frontend approval UI.
///
/// Flow:
///   Claude Code (hook) → POST /permission → server holds connection →
///   emits claude/requestApproval event → frontend shows toast →
///   user approves/declines → Tauri command resolves oneshot →
///   server returns PreToolUse permissionDecision allow/deny →
///   hook exits → Claude Code proceeds or stops.
///
/// AskUserQuestion calls are emitted as `item/tool/requestUserInput` instead,
/// and the user's answers return to Claude as the tool's `updatedInput`.
pub(crate) struct ClaudePermissionServer {
    pub(crate) port: u16,
    pub(crate) token: String,
    pending: PendingMap,
}

impl ClaudePermissionServer {
    pub(crate) async fn start<E: EventSink>(event_sink: E) -> Arc<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind Claude permission server");
        let port = listener.local_addr().unwrap().port();
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
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
        let outcome = if approved {
            ClaudeHookOutcome::Allow { updated_input: None }
        } else {
            ClaudeHookOutcome::Deny {
                reason: "Declined in Hopper".into(),
            }
        };
        if let Some(request) = self.pending.lock().await.remove(request_id) {
            let _ = request.sender.send(outcome);
        }
    }

    /// Called when the user answers an AskUserQuestion request. `answers` is
    /// the question card's `{ "<question id>": { "answers": [...] } }` map.
    pub(crate) async fn resolve_question(&self, request_id: &str, answers: &Value) {
        let Some(request) = self.pending.lock().await.remove(request_id) else {
            return;
        };
        let outcome = match request.questions_input {
            Some(input) => ClaudeHookOutcome::Allow {
                updated_input: Some(answered_question_input(&input, answers)),
            },
            None => ClaudeHookOutcome::Deny {
                reason: "Hopper received answers for a request that was not a question.".into(),
            },
        };
        let _ = request.sender.send(outcome);
    }
}

/// Maps AskUserQuestion input to the question card's request params. Question
/// ids are positional (`q0`, `q1`, …) because Claude's questions have none.
pub(crate) fn question_request_params(
    tool_input: &Value,
    thread_id: &str,
    turn_id: &str,
    item_id: &str,
) -> Option<Value> {
    let questions = tool_input.get("questions")?.as_array()?;
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
                        .map(|option| match option {
                            Value::String(label) => json!({ "label": label, "description": "" }),
                            _ => json!({
                                "label": option.get("label").and_then(Value::as_str).unwrap_or(""),
                                "description": option.get("description").and_then(Value::as_str).unwrap_or(""),
                            }),
                        })
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "id": format!("q{index}"),
                "header": question.get("header").and_then(Value::as_str).unwrap_or(""),
                "question": question.get("question").and_then(Value::as_str).unwrap_or(""),
                "options": options,
                "multiSelect": question.get("multiSelect").and_then(Value::as_bool).unwrap_or(false),
                "isOther": true,
            })
        })
        .collect();
    if mapped.is_empty() {
        return None;
    }
    Some(json!({
        "threadId": thread_id,
        "turnId": turn_id,
        "itemId": item_id,
        "questions": mapped,
    }))
}

/// Builds AskUserQuestion's `updatedInput`: the original questions plus an
/// `answers` map of question text → chosen labels (comma-joined) and notes.
pub(crate) fn answered_question_input(tool_input: &Value, answers: &Value) -> Value {
    let mut answer_map = serde_json::Map::new();
    let questions = tool_input
        .get("questions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (index, question) in questions.iter().enumerate() {
        let Some(text) = question.get("question").and_then(Value::as_str) else {
            continue;
        };
        let parts: Vec<String> = answers
            .get(format!("q{index}"))
            .and_then(|answer| answer.get("answers"))
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|value| value.strip_prefix("user_note:").unwrap_or(value).trim().to_string())
                    .filter(|value| !value.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        if !parts.is_empty() {
            answer_map.insert(text.to_string(), json!(parts.join(", ")));
        }
    }
    let mut updated = tool_input.as_object().cloned().unwrap_or_default();
    updated.insert("answers".into(), Value::Object(answer_map));
    Value::Object(updated)
}

async fn handle_connection<E: EventSink>(
    mut stream: TcpStream,
    pending: PendingMap,
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
    let query: HashMap<String, String> = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| reqwest::Url::parse(&format!("http://localhost{path}")).ok())
        .map(|url| url.query_pairs().into_owned().collect())
        .unwrap_or_default();
    let thread_id = query.get("thread_id").cloned().unwrap_or_default();
    let turn_id = query.get("turn_id").cloned().unwrap_or_default();
    let Some(workspace_id) = query.get("workspace_id").cloned() else {
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

    let tool_name = data
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let tool_input = data.get("tool_input").cloned().unwrap_or(json!({}));

    if tool_name == ASK_USER_QUESTION_TOOL {
        let tool_use_id = data.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
        let outcome =
            ask_question(&pending, &event_sink, &workspace_id, &thread_id, &turn_id, tool_use_id, tool_input)
                .await;
        let response_body = claude_permission_response(&outcome).to_string();
        let _ = write_http_response(&mut stream, 200, &response_body).await;
        return;
    }

    let request_id = format!("claude-{}", Uuid::new_v4());

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
    let (tx, rx) = oneshot::channel::<ClaudeHookOutcome>();
    pending.lock().await.insert(
        request_id.clone(),
        PendingRequest {
            sender: tx,
            questions_input: None,
        },
    );

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
    // Default to block on timeout for safety.
    let outcome = wait_for_outcome(&pending, &request_id, rx, APPROVAL_TIMEOUT, "Declined or timed out in Hopper").await;
    let response_body = claude_permission_response(&outcome).to_string();
    let _ = write_http_response(&mut stream, 200, &response_body).await;
}

/// Shows an AskUserQuestion call as a question card and waits for answers.
async fn ask_question<E: EventSink>(
    pending: &PendingMap,
    event_sink: &E,
    workspace_id: &str,
    thread_id: &str,
    turn_id: &str,
    tool_use_id: &str,
    tool_input: Value,
) -> ClaudeHookOutcome {
    let item_id = if tool_use_id.is_empty() {
        String::new()
    } else {
        format!("tool-{tool_use_id}")
    };
    let params = (!thread_id.is_empty())
        .then(|| question_request_params(&tool_input, thread_id, turn_id, &item_id))
        .flatten();
    let Some(params) = params else {
        return ClaudeHookOutcome::Deny {
            reason: "Hopper could not show this question. Ask it in your reply text instead.".into(),
        };
    };
    let request_id = format!("claude-q-{}", Uuid::new_v4());
    let (tx, rx) = oneshot::channel::<ClaudeHookOutcome>();
    pending.lock().await.insert(
        request_id.clone(),
        PendingRequest {
            sender: tx,
            questions_input: Some(tool_input),
        },
    );
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.to_string(),
        message: json!({
            "method": "item/tool/requestUserInput",
            "id": request_id,
            "params": params,
        }),
    });
    wait_for_outcome(pending, &request_id, rx, QUESTION_TIMEOUT, "The user did not answer in Hopper.").await
}

async fn wait_for_outcome(
    pending: &PendingMap,
    request_id: &str,
    rx: oneshot::Receiver<ClaudeHookOutcome>,
    timeout: std::time::Duration,
    fallback_reason: &str,
) -> ClaudeHookOutcome {
    let outcome = match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(outcome)) => outcome,
        _ => ClaudeHookOutcome::Deny {
            reason: fallback_reason.to_string(),
        },
    };
    // Remove the pending entry in case it wasn't consumed (e.g. timeout path).
    pending.lock().await.remove(request_id);
    outcome
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
                    "thread-1",
                    "turn-1",
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

    fn ask_input() -> Value {
        json!({"questions": [
            {"question": "Which format?", "header": "Format", "multiSelect": false,
             "options": [{"label": "JSON", "description": "Structured"}, {"label": "YAML", "description": "Readable"}]},
            {"question": "Which targets?", "header": "Targets", "multiSelect": true,
             "options": [{"label": "Linux", "description": ""}, {"label": "macOS", "description": ""}]}
        ]})
    }

    #[test]
    fn maps_ask_user_question_to_question_card_params() {
        let params = question_request_params(&ask_input(), "thread-1", "turn-1", "tool-abc").unwrap();
        assert_eq!(params["threadId"], "thread-1");
        assert_eq!(params["itemId"], "tool-abc");
        let questions = params["questions"].as_array().unwrap();
        assert_eq!(questions[0]["id"], "q0");
        assert_eq!(questions[0]["header"], "Format");
        assert_eq!(questions[0]["options"][1]["label"], "YAML");
        assert_eq!(questions[0]["multiSelect"], false);
        assert_eq!(questions[1]["multiSelect"], true);
        assert!(question_request_params(&json!({}), "t", "u", "i").is_none());
    }

    #[test]
    fn builds_updated_input_from_card_answers() {
        let updated = answered_question_input(
            &ask_input(),
            &json!({
                "q0": {"answers": ["YAML", "user_note: with comments"]},
                "q1": {"answers": ["Linux", "macOS"]},
            }),
        );
        assert_eq!(updated["questions"], ask_input()["questions"]);
        assert_eq!(updated["answers"]["Which format?"], "YAML, with comments");
        assert_eq!(updated["answers"]["Which targets?"], "Linux, macOS");
    }

    #[test]
    fn routes_ask_user_question_to_the_question_card_and_returns_answers() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                let server = ClaudePermissionServer::start(TestSink(tx)).await;
                let settings = crate::shared::provider_setup_core::claude_hook_settings(
                    server.port, &server.token, "ws", "thread-9", "turn-3",
                );
                let url = settings["hooks"]["PreToolUse"][0]["hooks"][0]["url"]
                    .as_str()
                    .unwrap()
                    .to_string();
                let token = server.token.clone();
                let response = tokio::spawn(async move {
                    reqwest::Client::new()
                        .post(&url)
                        .header("X-Hopper-Token", token)
                        .json(&json!({"tool_name": "AskUserQuestion", "tool_use_id": "toolu_1", "tool_input": ask_input()}))
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
                assert_eq!(event.message["method"], "item/tool/requestUserInput");
                assert_eq!(event.message["params"]["threadId"], "thread-9");
                assert_eq!(event.message["params"]["turnId"], "turn-3");
                assert_eq!(event.message["params"]["itemId"], "tool-toolu_1");
                let request_id = event.message["id"].as_str().unwrap().to_string();
                assert!(request_id.starts_with("claude-q-"));
                server
                    .resolve_question(&request_id, &json!({"q0": {"answers": ["JSON"]}}))
                    .await;
                let output = response.await.unwrap();
                assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "allow");
                assert_eq!(
                    output["hookSpecificOutput"]["updatedInput"]["answers"]["Which format?"],
                    "JSON"
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
