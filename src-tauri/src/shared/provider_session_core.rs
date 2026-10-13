//! Durable local CLI thread identities and completed conversation items.
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::backend::events::{AppServerEvent, EventSink, TerminalExit, TerminalOutput};

const MAX_HISTORY_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderThread {
    pub(crate) thread_id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) created_at: i64,
    #[serde(default)]
    pub(crate) turns: Vec<Value>,
    #[serde(default)]
    pub(crate) preview: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoreData {
    version: u32,
    workspaces: HashMap<String, HashMap<String, ProviderThread>>,
}

impl Default for StoreData {
    fn default() -> Self {
        Self {
            version: 1,
            workspaces: HashMap::new(),
        }
    }
}

pub(crate) struct ProviderSessionStore {
    path: Option<PathBuf>,
    data: Mutex<StoreData>,
    write_error: Mutex<Option<String>>,
    _lock: Option<HistoryLock>,
}

struct HistoryLock(std::fs::File);

impl Drop for HistoryLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // Release even when loading fails: a concurrent fork can briefly
            // inherit the descriptor before its close-on-exec runs.
            unsafe {
                libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

impl ProviderSessionStore {
    #[cfg(test)]
    pub(crate) fn memory() -> Arc<Self> {
        Arc::new(Self {
            path: None,
            _lock: None,
            data: Mutex::new(StoreData::default()),
            write_error: Mutex::new(None),
        })
    }

    pub(crate) fn load(path: PathBuf) -> Result<Arc<Self>, String> {
        let store_lock = lock_store(&path)?;
        let mut data: StoreData = match std::fs::File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(MAX_HISTORY_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| Self::load_error(&path, error))?;
                if bytes.len() > MAX_HISTORY_BYTES {
                    return Err(Self::load_error(&path, "provider history exceeds the 128 MiB size limit; back up and reduce this file"));
                }
                serde_json::from_slice(&bytes).map_err(|error| Self::load_error(&path, error))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoreData::default(),
            Err(error) => return Err(Self::load_error(&path, error)),
        };
        if data.version != 1 {
            return Err(Self::load_error(
                &path,
                "unsupported provider history version",
            ));
        }
        for threads in data.workspaces.values_mut() {
            for (id, thread) in threads {
                if id != &thread.thread_id {
                    return Err(Self::load_error(
                        &path,
                        "thread identity does not match its saved key",
                    ));
                }
                // An earlier app instance can no longer deliver this turn's
                // events. Preserve its completed items without showing it busy.
                for turn in &mut thread.turns {
                    if !turn.is_object()
                        || turn
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|id| !id.is_empty())
                            .is_none()
                        || turn.get("items").and_then(Value::as_array).is_none()
                    {
                        return Err(Self::load_error(&path, "invalid saved conversation turn"));
                    }
                    if turn.get("status").and_then(Value::as_str) == Some("inProgress") {
                        turn["status"] = json!("interrupted");
                    }
                }
            }
        }
        Ok(Arc::new(Self {
            path: Some(path),
            _lock: Some(store_lock),
            data: Mutex::new(data),
            write_error: Mutex::new(None),
        }))
    }

    fn load_error(path: &std::path::Path, error: impl std::fmt::Display) -> String {
        format!("Cannot load provider conversations from {}: {error}. Repair or restore this file before restarting Hopper; it has not been replaced.", path.display())
    }

    fn update(
        &self,
        change: impl FnOnce(&mut StoreData) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut data = self.data.lock().unwrap_or_else(|error| error.into_inner());
        let mut next = data.clone();
        change(&mut next)?;
        if let Some(path) = &self.path {
            let saved = encode_history(&next, MAX_HISTORY_BYTES).and_then(|bytes| {
                crate::storage::write_storage_atomic(path, &bytes)
                    .map_err(|error| error.to_string())
            });
            if let Err(error) = saved {
                let message = format!("Cannot save provider conversations to {}: {error}. Free disk space or restore write access before sending another turn. If the history size limit was reached, back up and reduce the history file before restarting Hopper.", path.display());
                // Keep the current session identity in memory so a later save
                // retries the entire snapshot, including the failed update.
                *data = next;
                *self
                    .write_error
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(message.clone());
                return Err(message);
            }
        }
        *data = next;
        *self
            .write_error
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
        Ok(())
    }

    pub(crate) fn create_thread(
        &self,
        workspace: &str,
        thread_id: &str,
        created_at: i64,
    ) -> Result<(), String> {
        self.update(|data| {
            data.workspaces.entry(workspace.into()).or_default().insert(
                thread_id.into(),
                ProviderThread {
                    thread_id: thread_id.into(),
                    session_id: None,
                    created_at,
                    turns: Vec::new(),
                    preview: None,
                    name: None,
                },
            );
            Ok(())
        })
    }

    pub(crate) fn require_thread(&self, workspace: &str, thread_id: &str) -> Result<(), String> {
        let retry_pending = self
            .write_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_some();
        if retry_pending {
            self.update(|_| Ok(()))?;
        }
        self.thread(workspace, thread_id).map(|_| ()).ok_or_else(|| {
            "This provider conversation has no saved session in Hopper. Start a new conversation instead of resuming it without its context.".into()
        })
    }

    pub(crate) fn require_resumable_for_send(
        &self,
        workspace: &str,
        thread_id: &str,
    ) -> Result<(), String> {
        self.require_thread(workspace, thread_id)?;
        let thread = self
            .thread(workspace, thread_id)
            .ok_or("Provider conversation not found")?;
        if !thread.turns.is_empty()
            && thread
                .session_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .is_none()
        {
            return Err("This conversation has saved messages but no provider session ID. Its context cannot be resumed safely. Start a new conversation; the saved messages remain available.".into());
        }
        Ok(())
    }

    pub(crate) fn thread(&self, workspace: &str, thread_id: &str) -> Option<ProviderThread> {
        self.data
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .workspaces
            .get(workspace)?
            .get(thread_id)
            .cloned()
    }

    pub(crate) fn threads(&self, workspace: &str) -> Vec<ProviderThread> {
        self.data
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .workspaces
            .get(workspace)
            .map(|threads| threads.values().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) fn read_response(&self, workspace: &str, thread_id: &str) -> Result<Value, String> {
        self.require_thread(workspace, thread_id)?;
        let thread = self
            .thread(workspace, thread_id)
            .ok_or("Provider conversation not found")?;
        Ok(json!({"result": {"thread": {
            "id": thread.thread_id, "status": {"type": "idle"}, "createdAt": thread.created_at,
            "sessionId": thread.session_id, "preview": thread.preview, "name": thread.name, "turns": thread.turns,
        }, "items": []}}))
    }

    pub(crate) fn set_name(
        &self,
        workspace: &str,
        thread_id: &str,
        name: String,
    ) -> Result<(), String> {
        self.require_thread(workspace, thread_id)?;
        self.update(|data| {
            let thread = data
                .workspaces
                .get_mut(workspace)
                .and_then(|threads| threads.get_mut(thread_id))
                .ok_or("Provider conversation not found")?;
            thread.name = Some(name);
            Ok(())
        })
    }

    pub(crate) fn set_session_id(
        &self,
        workspace: &str,
        thread_id: &str,
        session: String,
    ) -> Result<(), String> {
        self.update(|data| {
            let thread = data
                .workspaces
                .get_mut(workspace)
                .and_then(|threads| threads.get_mut(thread_id))
                .ok_or("Provider conversation not found")?;
            thread.session_id = Some(session);
            Ok(())
        })
    }

    fn record_event(&self, event: &AppServerEvent) -> Result<(), String> {
        let method = event
            .message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !matches!(
            method,
            "turn/started" | "turn/completed" | "item/completed" | "error"
        ) {
            return Ok(());
        }
        let params = &event.message["params"];
        let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
            return Ok(());
        };
        let turn_id = params
            .get("turnId")
            .and_then(Value::as_str)
            .or_else(|| {
                params
                    .get("turn")
                    .and_then(|turn| turn.get("id"))
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                params
                    .get("item")
                    .and_then(|item| item.get("turnId"))
                    .and_then(Value::as_str)
            });
        let Some(turn_id) = turn_id else {
            return Ok(());
        };
        self.update(|data| {
            let Some(thread) = data
                .workspaces
                .get_mut(&event.workspace_id)
                .and_then(|threads| threads.get_mut(thread_id))
            else {
                return Ok(());
            };
            let index = thread
                .turns
                .iter()
                .position(|turn| turn["id"] == turn_id)
                .unwrap_or_else(|| {
                    thread
                        .turns
                        .push(json!({"id": turn_id, "status": "inProgress", "items": []}));
                    thread.turns.len() - 1
                });
            let turn = &mut thread.turns[index];
            if method == "error" {
                turn["status"] = json!("failed");
                turn["error"] = params.get("error").cloned().unwrap_or(Value::Null);
            } else if method == "turn/completed" {
                turn["status"] = params
                    .get("turn")
                    .and_then(|turn| turn.get("status"))
                    .cloned()
                    .unwrap_or_else(|| {
                        if turn["status"] == "failed" {
                            json!("failed")
                        } else {
                            json!("completed")
                        }
                    });
            } else if method == "item/completed" {
                if let Some(item) = params.get("item") {
                    if item.get("type").and_then(Value::as_str) == Some("userMessage")
                        && thread.preview.is_none()
                    {
                        thread.preview = item
                            .get("content")
                            .and_then(Value::as_array)
                            .and_then(|content| {
                                content
                                    .iter()
                                    .find_map(|block| block.get("text").and_then(Value::as_str))
                            })
                            .map(|text| text.chars().take(160).collect());
                    }
                    let items = turn["items"]
                        .as_array_mut()
                        .ok_or("Invalid saved provider turn items")?;
                    if let Some(index) = items
                        .iter()
                        .position(|existing| existing.get("id") == item.get("id"))
                    {
                        items[index] = item.clone();
                    } else {
                        items.push(item.clone());
                    }
                }
            }
            Ok(())
        })
    }
}

fn lock_store(path: &std::path::Path) -> Result<HistoryLock, String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| ProviderSessionStore::load_error(path, error))?;
    }
    let lock_path = path.with_extension("lock");
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let file = options.open(&lock_path).map_err(|error| format!("Cannot lock provider conversations at {}: {error}. Close other Hopper instances and check folder permissions before retrying.", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(format!("Cannot lock provider conversations at {}: {}. Another Hopper instance may be using this history; close it before restarting.", path.display(), std::io::Error::last_os_error()));
        }
    }
    Ok(HistoryLock(file))
}

fn encode_history(data: &StoreData, limit: usize) -> Result<Vec<u8>, String> {
    struct BoundedWriter {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for BoundedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other(
                    "provider history exceeds the 128 MiB size limit",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, data).map_err(|error| error.to_string())?;
    Ok(writer.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(method: &str, params: Value) -> AppServerEvent {
        AppServerEvent {
            workspace_id: "workspace".into(),
            message: json!({"method": method, "params": params}),
        }
    }

    #[test]
    fn reload_restores_session_identity_and_hydratable_conversation_history() {
        let dir = std::env::temp_dir().join(format!("hopper-provider-{}", uuid::Uuid::new_v4()));
        let path = dir.join("claude.json");
        let store = ProviderSessionStore::load(path.clone()).unwrap();
        store.create_thread("workspace", "thread", 123).unwrap();
        store
            .set_session_id("workspace", "thread", "provider-session".into())
            .unwrap();
        store
            .record_event(&event(
                "turn/started",
                json!({"threadId":"thread", "turn":{"id":"turn"}}),
            ))
            .unwrap();
        store.record_event(&event("item/completed", json!({"threadId":"thread", "item":{
            "id":"user", "type":"userMessage", "turnId":"turn", "content":[{"type":"text", "text":"Hello"}, {"type":"image", "url":"data:image/png;base64,AAA="}]
        }}))).unwrap();
        for text in ["partial", "Complete 😀"] {
            store
                .record_event(&event(
                    "item/completed",
                    json!({"threadId":"thread", "item":{
                        "id":"answer", "type":"agentMessage", "turnId":"turn", "text":text
                    }}),
                ))
                .unwrap();
        }
        store
            .record_event(&event(
                "turn/completed",
                json!({"threadId":"thread", "turnId":"turn"}),
            ))
            .unwrap();
        store
            .set_name("workspace", "thread", "Saved name".into())
            .unwrap();
        drop(store);
        let restored = ProviderSessionStore::load(path).unwrap();
        let response = restored.read_response("workspace", "thread").unwrap();
        let thread = &response["result"]["thread"];
        assert_eq!(thread["sessionId"], "provider-session");
        assert_eq!(thread["createdAt"], 123);
        assert_eq!(thread["preview"], "Hello");
        assert_eq!(thread["name"], "Saved name");
        assert_eq!(thread["turns"][0]["items"].as_array().unwrap().len(), 2);
        assert_eq!(thread["turns"][0]["items"][1]["text"], "Complete 😀");
        assert_eq!(
            thread["turns"][0]["items"][0]["content"][1]["url"],
            "data:image/png;base64,AAA="
        );
        assert!(restored.read_response("other-workspace", "thread").is_err());
        assert!(ProviderSessionStore::load(dir.join("antigravity.json"))
            .unwrap()
            .threads("workspace")
            .is_empty());
        drop(restored);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interrupted_turns_keep_completed_items_without_staying_busy_after_restart() {
        let dir = std::env::temp_dir().join(format!("hopper-provider-{}", uuid::Uuid::new_v4()));
        let path = dir.join("history.json");
        let store = ProviderSessionStore::load(path.clone()).unwrap();
        store.create_thread("workspace", "thread", 123).unwrap();
        store
            .record_event(&event(
                "turn/started",
                json!({"threadId":"thread", "turn":{"id":"turn"}}),
            ))
            .unwrap();
        drop(store);
        let restored = ProviderSessionStore::load(path).unwrap();
        assert_eq!(
            restored.read_response("workspace", "thread").unwrap()["result"]["thread"]["turns"][0]
                ["status"],
            "interrupted"
        );
        drop(restored);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_second_writer_cannot_overwrite_an_open_history_store() {
        let dir = std::env::temp_dir().join(format!("hopper-provider-{}", uuid::Uuid::new_v4()));
        let path = dir.join("history.json");
        let store = ProviderSessionStore::load(path.clone()).unwrap();
        store.create_thread("workspace", "thread", 0).unwrap();
        let saved = std::fs::read(&path).unwrap();
        assert!(ProviderSessionStore::load(path.clone())
            .err()
            .unwrap()
            .contains("Hopper instance"));
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        drop(store);
        let reopened = ProviderSessionStore::load(path).unwrap();
        assert!(reopened.require_thread("workspace", "thread").is_ok());
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_session_identity_only_allows_an_unused_thread_to_start() {
        let store = ProviderSessionStore::memory();
        store.create_thread("workspace", "thread", 0).unwrap();
        assert!(store
            .require_resumable_for_send("workspace", "thread")
            .is_ok());
        store
            .record_event(&event(
                "turn/started",
                json!({"threadId":"thread", "turnId":"turn"}),
            ))
            .unwrap();
        assert!(store
            .require_resumable_for_send("workspace", "thread")
            .unwrap_err()
            .contains("no provider session ID"));
        assert!(store.read_response("workspace", "thread").is_ok());
        store
            .set_session_id("workspace", "thread", "session".into())
            .unwrap();
        assert!(store
            .require_resumable_for_send("workspace", "thread")
            .is_ok());
    }

    #[test]
    fn failed_turn_keeps_its_error_when_completion_arrives() {
        let store = ProviderSessionStore::memory();
        store.create_thread("workspace", "thread", 0).unwrap();
        store.record_event(&event("error", json!({"threadId":"thread", "turnId":"turn", "error":{"message":"Sign in required"}}))).unwrap();
        store
            .record_event(&event(
                "turn/completed",
                json!({"threadId":"thread", "turnId":"turn"}),
            ))
            .unwrap();
        let response = store.read_response("workspace", "thread").unwrap();
        assert_eq!(response["result"]["thread"]["turns"][0]["status"], "failed");
        assert_eq!(
            response["result"]["thread"]["turns"][0]["error"]["message"],
            "Sign in required"
        );
    }

    #[test]
    fn history_serialization_refuses_oversized_snapshots() {
        let data = StoreData::default();
        let bytes = encode_history(&data, 1024).unwrap();
        assert!(encode_history(&data, bytes.len() - 1)
            .unwrap_err()
            .contains("size limit"));
        assert_eq!(encode_history(&data, bytes.len()).unwrap(), bytes);
    }

    #[test]
    fn corrupt_or_incompatible_history_is_preserved_and_reported() {
        let dir = std::env::temp_dir().join(format!("hopper-provider-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        for contents in [
            "invalid JSON",
            r#"{"version":2,"workspaces":{}}"#,
            r#"{"version":1,"workspaces":{"workspace":{"thread":{"threadId":"thread","sessionId":null,"createdAt":0,"turns":[null]}}}}"#,
        ] {
            std::fs::write(&path, contents).unwrap();
            let error = ProviderSessionStore::load(path.clone())
                .err()
                .expect("must refuse corrupt history");
            assert!(error.contains(&path.display().to_string()));
            assert!(error.contains("Repair or restore"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_save_blocks_new_turns_until_pending_session_can_be_saved() {
        let dir = std::env::temp_dir().join(format!("hopper-provider-{}", uuid::Uuid::new_v4()));
        let path = dir.join("history.json");
        let store = ProviderSessionStore::load(path.clone()).unwrap();
        store.create_thread("workspace", "thread", 123).unwrap();
        std::fs::rename(&path, dir.join("original.json")).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(store
            .set_session_id("workspace", "thread", "session".into())
            .is_err());
        assert!(store.require_thread("workspace", "thread").is_err());
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(dir.join("original.json"), &path).unwrap();
        assert!(store.require_thread("workspace", "thread").is_ok());
        drop(store);
        assert_eq!(
            ProviderSessionStore::load(path)
                .unwrap()
                .thread("workspace", "thread")
                .unwrap()
                .session_id
                .as_deref(),
            Some("session")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[derive(Clone)]
pub(crate) struct ProviderEventSink<E: EventSink> {
    sink: E,
    store: Arc<ProviderSessionStore>,
}

impl<E: EventSink> ProviderEventSink<E> {
    pub(crate) fn new(sink: E, store: Arc<ProviderSessionStore>) -> Self {
        Self { sink, store }
    }
}

impl<E: EventSink> EventSink for ProviderEventSink<E> {
    fn emit_app_server_event(&self, event: AppServerEvent) {
        if let Err(error) = self.store.record_event(&event) {
            self.sink.emit_app_server_event(AppServerEvent {
                workspace_id: event.workspace_id.clone(),
                message: json!({"method": "error", "params": {"threadId": event.message["params"]["threadId"], "error": {"message": error}, "willRetry": false}}),
            });
        }
        self.sink.emit_app_server_event(event);
    }
    fn emit_terminal_output(&self, event: TerminalOutput) {
        self.sink.emit_terminal_output(event);
    }
    fn emit_terminal_exit(&self, event: TerminalExit) {
        self.sink.emit_terminal_exit(event);
    }
}
