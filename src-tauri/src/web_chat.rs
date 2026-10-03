//! Desktop-only conversation transfer between embedded assistant tabs and Hopper.
//!
//! Hopper asks an assistant tab for its conversation (or to paste text into its
//! composer) by evaluating the injected `web_chat.js` with a request id. The page
//! answers through `web_chat_reply`, the only command assistant tabs may call.
//! Replies are accepted only for request ids Hopper issued to that same tab.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use tauri::ipc::Invoke;
use tauri::{AppHandle, Emitter, Manager, Runtime, State, Url, Webview};
use tokio::sync::oneshot;
use uuid::Uuid;

pub(crate) const ASSISTANT_WEBVIEW_PREFIX: &str = "ai-chatbot-";
pub(crate) const WEB_CHAT_SCRIPT: &str = include_str!("web_chat/web_chat.js");

const ASSISTANT_HOSTS: &[(&str, &str)] = &[
    ("chatgpt.com", "ChatGPT"),
    ("chat.openai.com", "ChatGPT"),
    ("claude.ai", "Claude"),
    ("gemini.google.com", "Gemini"),
    ("copilot.microsoft.com", "Copilot"),
    ("chat.mistral.ai", "Mistral"),
];
const ASSISTANT_COMMANDS: &[&str] = &["web_chat_reply", "web_chat_file_import"];
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);
const INSERT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MESSAGES: usize = 10_000;
const MAX_CONVERSATION_BYTES: usize = 8 * 1024 * 1024;
const MAX_INSERT_BYTES: usize = 1024 * 1024;
const MAX_FILE_BYTES: usize = 25 * 1024 * 1024;
/// Imported files older than this are removed the next time a file is imported.
const IMPORT_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const FALLBACK_FILE_NAME: &str = "web-chat-file";

pub(crate) fn is_assistant_webview(label: &str) -> bool {
    label.starts_with(ASSISTANT_WEBVIEW_PREFIX)
}

pub(crate) fn assistant_command_allowed(command: &str) -> bool {
    ASSISTANT_COMMANDS.contains(&command)
}

/// Wraps the app command handler so assistant webviews, which run third-party
/// site code, can only reach `ASSISTANT_COMMANDS`. The app defines no ACL
/// manifest, so Tauri does not restrict app commands by origin on its own.
pub(crate) fn guard_assistant_invoke<R: Runtime>(
    handler: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        if is_assistant_webview(invoke.message.webview_ref().label())
            && !assistant_command_allowed(invoke.message.command())
        {
            invoke
                .resolver
                .reject("This command is not available to assistant tabs.");
            return true;
        }
        handler(invoke)
    }
}

/// Returns the display name of the assistant served at `url`, if it is one.
fn assistant_provider(url: &Url) -> Option<&'static str> {
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?;
    ASSISTANT_HOSTS
        .iter()
        .find(|(allowed, _)| host == *allowed || host.ends_with(&format!(".{allowed}")))
        .map(|(_, provider)| *provider)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebChatConversation {
    provider: String,
    title: Option<String>,
    url: String,
    messages: Vec<WebChatMessage>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebChatImportedFile {
    path: String,
    file_name: String,
    mime_type: Option<String>,
    /// The assistant tab the file came from.
    webview_label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebChatFilePayload {
    file_name: String,
    #[serde(default)]
    mime_type: Option<String>,
    content_base64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapturedPage {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    messages: Vec<WebChatMessage>,
}

/// Builds the conversation from what the page captured. Provider and URL come
/// from the webview itself, never from the page.
fn normalize_capture(
    provider: &str,
    url: &Url,
    data: Value,
) -> Result<WebChatConversation, String> {
    let page: CapturedPage = serde_json::from_value(data)
        .map_err(|_| "The assistant page returned an unreadable conversation.".to_string())?;
    if page.messages.len() > MAX_MESSAGES {
        return Err(format!(
            "This conversation has more than {MAX_MESSAGES} messages."
        ));
    }
    let messages: Vec<WebChatMessage> = page
        .messages
        .into_iter()
        .filter(|message| matches!(message.role.as_str(), "user" | "assistant"))
        .map(|message| WebChatMessage {
            role: message.role,
            content: message.content.trim().to_string(),
        })
        .filter(|message| !message.content.is_empty())
        .collect();
    if messages.is_empty() {
        return Err("No messages were found in this conversation.".into());
    }
    let total: usize = messages.iter().map(|message| message.content.len()).sum();
    if total > MAX_CONVERSATION_BYTES {
        return Err("This conversation is too large to transfer.".into());
    }
    let title = page
        .title
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty());
    Ok(WebChatConversation {
        provider: provider.to_string(),
        title,
        url: url.to_string(),
        messages,
    })
}

struct PendingRequest {
    label: String,
    sender: oneshot::Sender<Result<Value, String>>,
}

#[derive(Default)]
pub(crate) struct WebChatRegistry {
    pending: Mutex<HashMap<String, PendingRequest>>,
}

impl WebChatRegistry {
    fn register(&self, label: &str) -> (String, oneshot::Receiver<Result<Value, String>>) {
        let (sender, receiver) = oneshot::channel();
        let request_id = Uuid::new_v4().to_string();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(
                request_id.clone(),
                PendingRequest {
                    label: label.to_string(),
                    sender,
                },
            );
        }
        (request_id, receiver)
    }

    fn cancel(&self, request_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(request_id);
        }
    }

    fn resolve(
        &self,
        request_id: &str,
        label: &str,
        outcome: Result<Value, String>,
    ) -> Result<(), String> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Web chat registry is unavailable.".to_string())?;
        match pending.get(request_id) {
            Some(request) if request.label == label => {}
            _ => return Err("Unknown web chat request.".into()),
        }
        let request = pending
            .remove(request_id)
            .ok_or_else(|| "Unknown web chat request.".to_string())?;
        let _ = request.sender.send(outcome);
        Ok(())
    }
}

/// Returns the assistant webview for `label` with the provider it is showing.
fn assistant_webview(app: &AppHandle, label: &str) -> Result<(Webview, &'static str, Url), String> {
    if !is_assistant_webview(label) {
        return Err("Not an assistant tab.".into());
    }
    let webview = app
        .get_webview(label)
        .ok_or_else(|| "The assistant tab is not open.".to_string())?;
    let url = webview.url().map_err(|error| error.to_string())?;
    let provider = assistant_provider(&url)
        .ok_or_else(|| "This tab is not a supported assistant chat.".to_string())?;
    Ok((webview, provider, url))
}

/// Evaluates `call(request_id)` in the assistant tab and waits for its reply.
async fn request_page(
    registry: &WebChatRegistry,
    webview: &Webview,
    timeout: Duration,
    call: impl FnOnce(&str) -> String,
) -> Result<Value, String> {
    let (request_id, receiver) = registry.register(webview.label());
    let script = call(&request_id);
    if let Err(error) = webview.eval(script) {
        registry.cancel(&request_id);
        return Err(error.to_string());
    }
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => Err("The assistant tab closed before it answered.".into()),
        Err(_) => {
            registry.cancel(&request_id);
            Err("The assistant tab did not respond. Reload the tab and try again.".into())
        }
    }
}

fn page_call(method: &str, args: &[&str]) -> String {
    format!(
        "window.__hopperWebChat&&window.__hopperWebChat.{method}({});",
        args.join(",")
    )
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// Reads the conversation open in an assistant tab.
#[tauri::command]
pub(crate) async fn web_chat_capture(
    app: AppHandle,
    registry: State<'_, WebChatRegistry>,
    label: String,
) -> Result<WebChatConversation, String> {
    let (webview, provider, url) = assistant_webview(&app, &label)?;
    let data = request_page(&registry, &webview, CAPTURE_TIMEOUT, |request_id| {
        page_call("capture", &[&js_string(request_id)])
    })
    .await?;
    normalize_capture(provider, &url, data)
}

/// Pastes `text` into the composer of an assistant tab without sending it.
#[tauri::command]
pub(crate) async fn web_chat_insert(
    app: AppHandle,
    registry: State<'_, WebChatRegistry>,
    label: String,
    text: String,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("There is nothing to paste.".into());
    }
    if text.len() > MAX_INSERT_BYTES {
        return Err("This Hopper conversation is too large to paste.".into());
    }
    let (webview, _, _) = assistant_webview(&app, &label)?;
    request_page(&registry, &webview, INSERT_TIMEOUT, |request_id| {
        page_call("insert", &[&js_string(request_id), &js_string(&text)])
    })
    .await
    .map(|_| ())
}

fn safe_file_name(value: &str) -> String {
    let source = Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(FALLBACK_FILE_NAME);
    let sanitized: String = source
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | ' ' => character,
            _ => '_',
        })
        .collect();
    let trimmed = sanitized.trim_matches([' ', '.']);
    // A name made only of replaced characters says nothing about the file.
    if trimmed.chars().any(|character| character.is_ascii_alphanumeric()) {
        trimmed.into()
    } else {
        FALLBACK_FILE_NAME.into()
    }
}

/// Removes imports older than `IMPORT_RETENTION`; they were already attached or abandoned.
async fn remove_stale_imports(directory: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(directory).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let is_stale = entry
            .metadata()
            .await
            .ok()
            .filter(|metadata| metadata.is_file())
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > IMPORT_RETENTION);
        if is_stale {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

/// Receives one user-selected downloadable file from an embedded assistant tab.
/// The page fetches the file in its signed-in browser context; Hopper stores it
/// in managed app data and adds the resulting local path to the Composer.
#[tauri::command]
pub(crate) async fn web_chat_file_import(
    app: AppHandle,
    webview: Webview,
    payload: WebChatFilePayload,
) -> Result<WebChatImportedFile, String> {
    let url = webview.url().map_err(|error| error.to_string())?;
    if !is_assistant_webview(webview.label()) || assistant_provider(&url).is_none() {
        return Err("Only supported assistant tabs can transfer files.".into());
    }
    if payload.content_base64.len() > MAX_FILE_BYTES.saturating_mul(2) {
        return Err("This file is too large to transfer (maximum 25 MB).".into());
    }
    let bytes = STANDARD
        .decode(payload.content_base64.as_bytes())
        .map_err(|_| "The assistant page returned an unreadable file.".to_string())?;
    if bytes.is_empty() {
        return Err("This file is empty.".into());
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err("This file is too large to transfer (maximum 25 MB).".into());
    }

    let file_name = safe_file_name(&payload.file_name);
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("web-chat-imports");
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| error.to_string())?;
    remove_stale_imports(&directory).await;
    let path = directory.join(format!("{}-{file_name}", Uuid::new_v4()));
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|error| error.to_string())?;
    let imported = WebChatImportedFile {
        path: path.to_string_lossy().into_owned(),
        file_name,
        mime_type: payload.mime_type.filter(|value| !value.trim().is_empty()),
        webview_label: webview.label().to_string(),
    };
    app.emit("web-chat-file-imported", &imported)
        .map_err(|error| error.to_string())?;
    Ok(imported)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebChatReply {
    request_id: String,
    ok: bool,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

/// Called by the injected page script to answer a Hopper request.
#[tauri::command]
pub(crate) fn web_chat_reply(
    webview: Webview,
    registry: State<'_, WebChatRegistry>,
    payload: WebChatReply,
) -> Result<(), String> {
    let url = webview.url().map_err(|error| error.to_string())?;
    if !is_assistant_webview(webview.label()) || assistant_provider(&url).is_none() {
        return Err("Only assistant tabs can answer web chat requests.".into());
    }
    let outcome = if payload.ok {
        Ok(payload.data.unwrap_or(Value::Null))
    } else {
        Err(payload
            .error
            .filter(|error| !error.trim().is_empty())
            .unwrap_or_else(|| "The assistant page could not complete the request.".into()))
    };
    registry.resolve(&payload.request_id, webview.label(), outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn url(value: &str) -> Url {
        Url::parse(value).unwrap()
    }

    #[test]
    fn restricts_assistant_webviews_to_reply_command() {
        assert!(is_assistant_webview("ai-chatbot-tab-1"));
        assert!(!is_assistant_webview("main"));
        assert!(assistant_command_allowed("web_chat_reply"));
        assert!(assistant_command_allowed("web_chat_file_import"));
        assert!(!assistant_command_allowed("web_chat_capture"));
        assert!(!assistant_command_allowed("web_chat_insert"));
        assert!(!assistant_command_allowed("file_write"));
    }

    #[test]
    fn recognizes_assistant_hosts_only_over_https() {
        let provider = |value: &str| assistant_provider(&url(value));
        assert_eq!(provider("https://chatgpt.com/c/1"), Some("ChatGPT"));
        assert_eq!(provider("https://chat.openai.com/c/1"), Some("ChatGPT"));
        assert_eq!(provider("https://www.claude.ai/"), Some("Claude"));
        assert_eq!(provider("http://claude.ai/"), None);
        assert_eq!(provider("https://claude.ai.evil.com/"), None);
        assert_eq!(provider("https://www.google.com/"), None);
    }

    #[test]
    fn normalizes_captured_conversation() {
        let conversation = normalize_capture(
            "Claude",
            &url("https://claude.ai/chat/abc"),
            json!({
                "title": "  Plan  ",
                "messages": [
                    { "role": "user", "content": " Hi " },
                    { "role": "system", "content": "hidden" },
                    { "role": "assistant", "content": "   " },
                    { "role": "assistant", "content": "Hello" },
                ],
            }),
        )
        .expect("conversation");
        assert_eq!(conversation.provider, "Claude");
        assert_eq!(conversation.title.as_deref(), Some("Plan"));
        assert_eq!(conversation.url, "https://claude.ai/chat/abc");
        assert_eq!(
            conversation.messages,
            vec![
                WebChatMessage { role: "user".into(), content: "Hi".into() },
                WebChatMessage { role: "assistant".into(), content: "Hello".into() },
            ]
        );
    }

    #[test]
    fn rejects_empty_or_oversized_captures() {
        let page = url("https://chatgpt.com/c/1");
        assert!(normalize_capture("ChatGPT", &page, json!({ "messages": [] })).is_err());
        assert!(normalize_capture("ChatGPT", &page, json!("nope")).is_err());
        let huge = "x".repeat(MAX_CONVERSATION_BYTES + 1);
        assert!(normalize_capture(
            "ChatGPT",
            &page,
            json!({ "messages": [{ "role": "user", "content": huge }] }),
        )
        .is_err());
    }

    #[test]
    fn sanitizes_imported_file_names() {
        assert_eq!(safe_file_name("../../report.pdf"), "report.pdf");
        assert_eq!(safe_file_name("  <>  "), "web-chat-file");
        assert_eq!(safe_file_name("review final?.docx"), "review final_.docx");
    }

    #[test]
    fn resolves_only_matching_requests() {
        let registry = WebChatRegistry::default();
        let (request_id, mut receiver) = registry.register("ai-chatbot-tab-1");
        assert!(registry
            .resolve(&request_id, "ai-chatbot-tab-2", Ok(Value::Null))
            .is_err());
        assert!(registry
            .resolve("unknown", "ai-chatbot-tab-1", Ok(Value::Null))
            .is_err());
        registry
            .resolve(&request_id, "ai-chatbot-tab-1", Ok(json!(1)))
            .expect("resolve");
        assert_eq!(receiver.try_recv().expect("reply"), Ok(json!(1)));
        assert!(registry
            .resolve(&request_id, "ai-chatbot-tab-1", Ok(Value::Null))
            .is_err());
    }

    #[test]
    fn cancelled_requests_cannot_be_resolved() {
        let registry = WebChatRegistry::default();
        let (request_id, _receiver) = registry.register("ai-chatbot-tab-1");
        registry.cancel(&request_id);
        assert!(registry
            .resolve(&request_id, "ai-chatbot-tab-1", Ok(Value::Null))
            .is_err());
    }

    #[test]
    fn page_calls_embed_arguments_as_json_strings() {
        assert_eq!(
            page_call("insert", &[&js_string("id"), &js_string("a\"b\n</script>")]),
            "window.__hopperWebChat&&window.__hopperWebChat.insert(\"id\",\"a\\\"b\\n</script>\");"
        );
    }
}
