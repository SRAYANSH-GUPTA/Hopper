//! Resource and stream helpers shared by the CLI providers.

pub(crate) fn guard_remote_provider_method(
    provider: &crate::types::LocalAgentProvider,
    method: &str,
    params: &serde_json::Value,
) -> Result<(), String> {
    let requested_provider = if method == "send_user_message" {
        params
            .get("provider")
            .filter(|value| !value.is_null())
            .map(|value| serde_json::from_value::<crate::types::LocalAgentProvider>(value.clone()))
            .transpose()
            .map_err(|error| format!("Invalid remote provider: {error}"))?
    } else {
        None
    };
    let provider = requested_provider.as_ref().unwrap_or(provider);
    if !matches!(provider, crate::types::LocalAgentProvider::Codex)
        && matches!(
            method,
            "start_thread"
                | "resume_thread"
                | "read_thread"
                | "thread_live_subscribe"
                | "thread_live_unsubscribe"
                | "fork_thread"
                | "list_threads"
                | "archive_thread"
                | "compact_thread"
                | "set_thread_name"
                | "send_user_message"
                | "turn_interrupt"
                | "turn_steer"
                | "start_review"
                | "model_list"
                | "collaboration_mode_list"
                | "list_mcp_server_status"
                | "experimental_feature_list"
                | "account_rate_limits"
                | "account_read"
                | "skills_list"
                | "apps_list"
                | "respond_to_server_request"
        )
    {
        return Err("The remote daemon currently supports Codex conversations only. Select Codex for remote mode, or use local mode for Claude and Antigravity.".into());
    }
    Ok(())
}

pub(crate) fn write_private_temp_file(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    if let Err(error) = file.write_all(bytes) {
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct TemporaryFiles(Vec<String>);

impl TemporaryFiles {
    pub(crate) fn track(&mut self, path: String) {
        self.0.push(path);
    }
}

impl Drop for TemporaryFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[derive(Default)]
pub(crate) struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut output = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    output.push_str(text);
                    self.pending.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    output.push_str(&String::from_utf8_lossy(&self.pending[..valid]));
                    if let Some(invalid) = error.error_len() {
                        output.push('\u{fffd}');
                        self.pending.drain(..valid + invalid);
                    } else {
                        // Retain a split code point for the next read.
                        self.pending.drain(..valid);
                        break;
                    }
                }
            }
        }
        output
    }

    pub(crate) fn finish(&mut self) -> String {
        let remaining = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        remaining
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_calls_cannot_silently_select_a_different_provider() {
        use crate::types::LocalAgentProvider;
        for provider in [LocalAgentProvider::Claude, LocalAgentProvider::Antigravity] {
            for method in [
                "start_thread",
                "send_user_message",
                "list_threads",
                "turn_interrupt",
                "model_list",
            ] {
                assert!(
                    guard_remote_provider_method(&provider, method, &serde_json::Value::Null)
                        .unwrap_err()
                        .contains("Select Codex")
                );
            }
            assert!(guard_remote_provider_method(
                &provider,
                "list_workspaces",
                &serde_json::Value::Null
            )
            .is_ok());
            assert!(guard_remote_provider_method(
                &provider,
                "provider_setup_action",
                &serde_json::Value::Null
            )
            .is_ok());
        }
        assert!(guard_remote_provider_method(
            &LocalAgentProvider::Codex,
            "send_user_message",
            &serde_json::Value::Null
        )
        .is_ok());
    }

    #[test]
    fn remote_provider_override_is_validated_before_dispatch() {
        use crate::types::LocalAgentProvider;
        assert!(guard_remote_provider_method(
            &LocalAgentProvider::Codex,
            "send_user_message",
            &serde_json::json!({"provider":"claude"})
        )
        .is_err());
        assert!(guard_remote_provider_method(
            &LocalAgentProvider::Codex,
            "send_user_message",
            &serde_json::json!({"provider":"unknown"})
        )
        .is_err());
        assert!(guard_remote_provider_method(
            &LocalAgentProvider::Claude,
            "send_user_message",
            &serde_json::json!({"provider":"codex"})
        )
        .is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_stops_every_registered_process_group_and_rejects_late_starts() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let processes = ProviderProcesses::default();
                let mut children = Vec::new();
                for _ in 0..2 {
                    let mut command = crate::shared::process_core::tokio_command("sh");
                    command
                        .args(["-c", "sleep 60 & echo ready; wait"])
                        .stdout(std::process::Stdio::piped());
                    crate::shared::process_core::configure_owned_process(&mut command);
                    let mut child = command.spawn().unwrap();
                    let pid = child.id().unwrap();
                    let mut stdout = BufReader::new(child.stdout.take().unwrap());
                    let mut ready = String::new();
                    stdout.read_line(&mut ready).await.unwrap();
                    assert_eq!(ready.trim(), "ready");
                    processes.register("thread".into(), pid).await.unwrap();
                    children.push((child, stdout, pid));
                }
                processes.shutdown().await;
                for (mut child, mut stdout, pid) in children {
                    let closed = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        stdout.read_to_end(&mut Vec::new()),
                    )
                    .await;
                    if closed.is_err() {
                        crate::shared::process_core::kill_registered_process_tree(pid).await;
                    }
                    closed
                        .expect("shutdown must stop descendants holding stdout open")
                        .unwrap();
                    child.wait().await.unwrap();
                }
                let mut command = crate::shared::process_core::tokio_command("sleep");
                command.arg("60");
                crate::shared::process_core::configure_owned_process(&mut command);
                let mut late = command.spawn().unwrap();
                assert!(processes
                    .register("late".into(), late.id().unwrap())
                    .await
                    .unwrap_err()
                    .contains("shutting down"));
                tokio::time::timeout(std::time::Duration::from_secs(2), late.wait())
                    .await
                    .unwrap()
                    .unwrap();
            });
    }

    #[test]
    fn utf8_survives_every_possible_split_boundary() {
        let expected = "Hello 😀 中文 हिन्दी café";
        for size in 1..expected.len() {
            let mut decoder = Utf8StreamDecoder::default();
            let mut output = String::new();
            for chunk in expected.as_bytes().chunks(size) {
                output.push_str(&decoder.push(chunk));
            }
            output.push_str(&decoder.finish());
            assert_eq!(output, expected);
        }
    }

    #[test]
    fn invalid_and_incomplete_utf8_matches_standard_lossy_decoding() {
        let bytes = [0xf0, 0x28, 0x8c, 0xbc, b'A', 0xf0, 0x9f];
        let mut decoder = Utf8StreamDecoder::default();
        let mut output = String::new();
        for byte in bytes {
            output.push_str(&decoder.push(&[byte]));
        }
        output.push_str(&decoder.finish());
        assert_eq!(output, String::from_utf8_lossy(&bytes));
    }

    #[cfg(unix)]
    #[test]
    fn temporary_images_are_private_and_never_replace_existing_files() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("hopper-image-{}", uuid::Uuid::new_v4()));
        write_private_temp_file(&path, b"original").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(write_private_temp_file(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn temporary_images_are_removed_on_an_early_failure() {
        let path = std::env::temp_dir().join(format!("hopper-image-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"private image").unwrap();
        let fail = || -> Result<(), String> {
            let mut files = TemporaryFiles::default();
            files.track(path.to_string_lossy().into_owned());
            Err("provider could not start".into())
        };
        assert!(fail().is_err());
        assert!(!path.exists());
    }
}

/// All live CLI children, including overlapping turns, remain tracked until
/// their wait task finishes. Exit closes registration before draining them.
#[derive(Default)]
pub(crate) struct ProviderProcesses {
    stopping: std::sync::atomic::AtomicBool,
    pids: tokio::sync::Mutex<std::collections::HashMap<String, std::collections::HashSet<u32>>>,
}

impl ProviderProcesses {
    pub(crate) async fn register(&self, key: String, pid: u32) -> Result<(), String> {
        let mut pids = self.pids.lock().await;
        if self.stopping.load(std::sync::atomic::Ordering::SeqCst) {
            drop(pids);
            crate::shared::process_core::kill_registered_process_tree(pid).await;
            return Err(
                "Hopper is shutting down. Wait for it to close before starting another turn."
                    .into(),
            );
        }
        pids.entry(key).or_default().insert(pid);
        Ok(())
    }

    pub(crate) async fn unregister(&self, key: &str, pid: Option<u32>) {
        let mut pids = self.pids.lock().await;
        if let Some(entries) = pids.get_mut(key) {
            if let Some(pid) = pid {
                entries.remove(&pid);
            }
            if entries.is_empty() {
                pids.remove(key);
            }
        }
    }

    pub(crate) async fn interrupt(&self, key: &str) {
        let pids: Vec<u32> = self
            .pids
            .lock()
            .await
            .get(key)
            .map(|pids| pids.iter().copied().collect())
            .unwrap_or_default();
        for pid in pids {
            crate::shared::process_core::terminate_process(pid).await;
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.stopping
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let pids: Vec<u32> = self
            .pids
            .lock()
            .await
            .drain()
            .flat_map(|(_, pids)| pids)
            .collect();
        for pid in pids {
            crate::shared::process_core::kill_registered_process_tree(pid).await;
        }
    }

    #[cfg(test)]
    pub(crate) async fn has_pid(&self, key: &str, pid: u32) -> bool {
        self.pids
            .lock()
            .await
            .get(key)
            .map(|pids| pids.contains(&pid))
            .unwrap_or(false)
    }
}
