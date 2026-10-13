//! Bounded Claude control-protocol queries. Initialization never submits a model prompt.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use super::process_core::{configure_owned_process, kill_child_process_tree, tokio_command};

const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const INITIALIZE_ID: &str = "hopper-command-discovery";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeCommand {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) argument_hint: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
}

fn valid_name(name: &str) -> bool {
    name.len() <= 160
        && name
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_:.-".contains(&byte))
}

/// Absolute paths such as /tmp/file remain ordinary prompts.
pub(crate) fn leading_command(text: &str) -> Option<&str> {
    let token = text
        .trim_start()
        .strip_prefix('/')?
        .split_whitespace()
        .next()?;
    (!token.contains('/')).then_some(token)
}

pub(crate) fn ensure_supported(text: &str, commands: &[ClaudeCommand]) -> Result<(), String> {
    let Some(name) = leading_command(text) else {
        return Ok(());
    };
    if commands
        .iter()
        .any(|command| command.name == name || command.aliases.iter().any(|alias| alias == name))
    {
        Ok(())
    } else {
        Err(format!("/{name} is not available in this Claude session. Use /help for Hopper commands or open Claude Code in a terminal. Your text was not sent to the model."))
    }
}

pub(crate) fn permission_mode(
    access_mode: Option<&str>,
    collaboration_mode: Option<&Value>,
) -> Result<&'static str, String> {
    if collaboration_mode
        .and_then(|mode| mode.get("mode"))
        .and_then(Value::as_str)
        == Some("plan")
    {
        return Ok("plan");
    }
    match access_mode.unwrap_or("current") {
        "read-only" => Ok("plan"),
        "current" => Ok("default"),
        "full-access" => Ok("bypassPermissions"),
        _ => Err("Unsupported Claude access mode".into()),
    }
}

fn parse_commands(value: &Value) -> Result<Vec<ClaudeCommand>, String> {
    let entries = value
        .as_array()
        .ok_or("Claude did not return a command registry. Update Claude Code and try again.")?;
    if entries.len() > 4096 {
        return Err("Claude command registry is too large".into());
    }
    entries
        .iter()
        .map(|entry| {
            let mut command: ClaudeCommand = serde_json::from_value(entry.clone())
                .map_err(|_| "Invalid Claude command registry")?;
            if !valid_name(&command.name) || command.aliases.iter().any(|alias| !valid_name(alias))
            {
                return Err("Invalid Claude command name".into());
            }
            command.description = command.description.chars().take(4096).collect();
            command.argument_hint = command.argument_hint.chars().take(1024).collect();
            Ok(command)
        })
        .collect()
}

fn discovery_response(output: &str) -> Result<Vec<ClaudeCommand>, String> {
    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if event.get("type").and_then(Value::as_str) != Some("control_response") {
            continue;
        }
        let response = &event["response"];
        if response["request_id"].as_str() != Some(INITIALIZE_ID) {
            continue;
        }
        if response["subtype"].as_str() != Some("success") {
            return Err(
                "Claude command discovery failed. Update Claude Code and try again.".into(),
            );
        }
        return parse_commands(&response["response"]["commands"]);
    }
    Err("Claude did not return a command registry. Update Claude Code and try again.".into())
}

/// Built-ins can finish entirely in result.result, with no assistant message.
pub(crate) fn result_text(event: &Value, has_assistant_text: bool) -> Option<String> {
    if has_assistant_text
        || event["type"].as_str() != Some("result")
        || event["is_error"].as_bool() == Some(true)
        || event["subtype"].as_str() != Some("success")
    {
        return None;
    }
    event["result"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

pub(crate) fn compact_notice(event: &Value) -> Option<String> {
    if event["type"].as_str() != Some("system")
        || event["subtype"].as_str() != Some("compact_boundary")
    {
        return None;
    }
    Some(match event["compact_metadata"]["pre_tokens"].as_u64() {
        Some(tokens) => {
            format!("Claude compacted the conversation (previous context: {tokens} tokens).")
        }
        None => "Claude compacted the conversation.".to_owned(),
    })
}

async fn read_bounded(reader: impl AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("Claude command output exceeded the size limit".into());
    }
    Ok(bytes)
}

// Preserve group ownership until pipe collection and child reaping both finish.
struct ProcessGroupGuard(Option<u32>);
impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.filter(|pid| *pid > 0 && *pid <= i32::MAX as u32) {
            unsafe {
                libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
            }
        }
    }
}

async fn run_control_query(
    binary: &str,
    cwd: Option<&str>,
    input: &[u8],
    mode: &str,
    timeout: Duration,
) -> Result<String, String> {
    let mut command = tokio_command(binary);
    configure_owned_process(&mut command);
    command.args([
        "-p",
        "--verbose",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--no-session-persistence",
        "--permission-mode",
        mode,
        "--settings",
        "{\"disableAllHooks\":true}",
        "--strict-mcp-config",
        "--mcp-config",
        "{\"mcpServers\":{}}",
        "--tools",
        "",
    ]);
    if let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty()) {
        command.current_dir(cwd);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("Cannot start Claude command discovery: {error}"))?;
    let mut group = ProcessGroupGuard(child.id());
    let mut stdin = child.stdin.take().ok_or("Claude stdin is unavailable")?;
    let stdout = child.stdout.take().ok_or("Claude stdout is unavailable")?;
    let stderr = child.stderr.take().ok_or("Claude stderr is unavailable")?;
    let outcome = tokio::time::timeout(timeout, async {
        let write = async {
            stdin
                .write_all(input)
                .await
                .map_err(|error| error.to_string())?;
            drop(stdin);
            Ok::<(), String>(())
        };
        let (_, stdout, _) = futures_util::future::try_join3(
            write,
            read_bounded(stdout, MAX_OUTPUT_BYTES),
            read_bounded(stderr, 64 * 1024),
        )
        .await?;
        let status = child.wait().await.map_err(|error| error.to_string())?;
        group.0 = None;
        if !status.success() {
            return Err(
                "Claude command query failed. Check your Claude Code installation and sign-in."
                    .into(),
            );
        }
        String::from_utf8(stdout).map_err(|_| "Claude returned invalid command output".into())
    })
    .await;
    drop(group);
    kill_child_process_tree(&mut child).await;
    let _ = child.wait().await;
    outcome
        .map_err(|_| "Claude command query timed out; no prompt was sent to the model".to_owned())?
}

pub(crate) async fn discover_commands(
    binary: &str,
    cwd: Option<&str>,
    mode: &str,
) -> Result<Vec<ClaudeCommand>, String> {
    let mut request = serde_json::to_vec(&json!({"type":"control_request", "request_id":INITIALIZE_ID, "request":{"subtype":"initialize", "hooks":{}}})).map_err(|error| error.to_string())?;
    request.push(b'\n');
    discovery_response(&run_control_query(binary, cwd, &request, mode, COMMAND_TIMEOUT).await?)
}

pub(crate) async fn usage_report(binary: &str, cwd: Option<&str>) -> Result<String, String> {
    let commands = discover_commands(binary, cwd, "default").await?;
    ensure_supported("/usage", &commands)?;
    let mut request = serde_json::to_vec(&json!({"type":"user", "message":{"role":"user", "content":"/usage"}, "parent_tool_use_id":null})).map_err(|error| error.to_string())?;
    request.push(b'\n');
    let output = run_control_query(binary, cwd, &request, "default", COMMAND_TIMEOUT).await?;
    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(text) = result_text(&event, false) {
            return Ok(format!("{text}\n\nThis report uses a separate Claude Code query session. Any session cost shown above is for that query, not this Hopper conversation. Account limits are shown only when Claude Code supplies them."));
        }
        if event["type"].as_str() == Some("result") && event["is_error"].as_bool() == Some(true) {
            return Err("Claude could not provide usage. Check your Claude Code sign-in.".into());
        }
    }
    Err("Claude Code did not provide a usage report. Open /usage in its terminal for account limits; Hopper displays thread tokens separately.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Vec<ClaudeCommand> {
        discovery_response(&json!({"type":"control_response","response":{"subtype":"success","request_id":INITIALIZE_ID,"response":{"commands":[{"name":"compact"},{"name":"context"},{"name":"mcp"},{"name":"agents"},{"name":"usage","aliases":["cost","stats"]},{"name":"123-review"},{"name":"plugin:review","description":"Review changes"},{"name":"__remote-workflow"}]}}}).to_string()).unwrap()
    }

    #[test]
    fn registry_checks_exact_names_and_preserves_paths() {
        let commands = registry();
        for text in [
            "/compact focus",
            "/context",
            "/mcp",
            "/agents",
            "/cost",
            "/plugin:review",
            "/123-review",
        ] {
            assert!(ensure_supported(text, &commands).is_ok());
        }
        for text in [
            "/help",
            "/unknown",
            "/unknown!",
            "/usage:other",
            "/compact-other",
        ] {
            assert!(ensure_supported(text, &commands).is_err());
        }
        for text in [
            "usage of this",
            "/tmp/file",
            "Review /compact",
            "//server/path",
        ] {
            assert_eq!(leading_command(text), None);
        }
    }

    #[test]
    fn discovery_fails_closed_on_missing_or_invalid_registry() {
        for text in ["", "not json", "{\"type\":\"result\",\"result\":\"hello\"}"] {
            assert!(discovery_response(text).is_err());
        }
        assert!(parse_commands(&json!([{"name":"../file"}])).is_err());
    }

    #[test]
    fn real_cli_initialize_registry_accepts_internal_and_public_commands() {
        // Claude Code 2.1.296 initialize response from an empty home, with no
        // credentials, network, or user prompt. Only names/aliases retained.
        let commands =
            discovery_response(include_str!("fixtures/claude_initialize_commands.json")).unwrap();
        for command in [
            "/compact", "/context", "/mcp", "/agents", "/usage", "/cost", "/stats",
        ] {
            assert!(ensure_supported(command, &commands).is_ok(), "{command}");
        }
        for command in [
            "/plugin",
            "/plugins",
            "/plan",
            "/help",
            "/permissions",
            "/skills",
            "/status",
            "/unknown!",
        ] {
            assert!(ensure_supported(command, &commands).is_err(), "{command}");
        }
    }

    #[test]
    fn plan_takes_precedence_over_full_access() {
        assert_eq!(
            permission_mode(Some("full-access"), Some(&json!({"mode":"plan"}))).unwrap(),
            "plan"
        );
        assert_eq!(permission_mode(Some("read-only"), None).unwrap(), "plan");
        assert_eq!(permission_mode(None, None).unwrap(), "default");
        assert_eq!(
            permission_mode(Some("full-access"), None).unwrap(),
            "bypassPermissions"
        );
        assert!(permission_mode(Some("unknown"), None).is_err());
    }

    #[test]
    fn result_only_commands_are_visible_without_duplicate_assistant_text() {
        let event =
            json!({"type":"result","subtype":"success","result":"Not enough messages to compact."});
        assert_eq!(
            result_text(&event, false).as_deref(),
            Some("Not enough messages to compact.")
        );
        assert_eq!(result_text(&event, true), None);
        assert_eq!(
            result_text(
                &json!({"type":"result","subtype":"error","result":"bad"}),
                false
            ),
            None
        );
        assert_eq!(compact_notice(&json!({"type":"system","subtype":"compact_boundary","compact_metadata":{"pre_tokens":123}})).unwrap(), "Claude compacted the conversation (previous context: 123 tokens).");
    }

    #[cfg(unix)]
    #[test]
    fn hung_query_is_killed_within_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let path =
            std::env::temp_dir().join(format!("hopper-claude-timeout-{}.sh", uuid::Uuid::new_v4()));
        std::fs::write(&path, "#!/bin/sh\nsleep 60\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let start = std::time::Instant::now();
                let result = run_control_query(
                    path.to_str().unwrap(),
                    None,
                    b"{}\n",
                    "default",
                    Duration::from_millis(100),
                )
                .await;
                assert!(result.unwrap_err().contains("timed out"));
                assert!(start.elapsed() < Duration::from_secs(3));
            });
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn command_output_limit_is_enforced() {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                assert!(read_bounded(&b"12345"[..], 4)
                    .await
                    .unwrap_err()
                    .contains("size limit"));
                assert_eq!(read_bounded(&b"1234"[..], 4).await.unwrap(), b"1234");
            });
    }

    #[cfg(unix)]
    fn process_running(pid: u32) -> bool {
        #[cfg(target_os = "linux")]
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            if stat
                .split_once(") ")
                .is_some_and(|(_, rest)| rest.starts_with('Z'))
            {
                return false;
            }
        }
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    #[cfg(unix)]
    #[test]
    fn timeout_and_cancellation_stop_descendants_holding_output_open() {
        use std::os::unix::fs::PermissionsExt;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                for cancel in [false, true] {
                    let root = std::env::temp_dir()
                        .join(format!("hopper-claude-descendant-{}", uuid::Uuid::new_v4()));
                    std::fs::create_dir(&root).unwrap();
                    let script = root.join("fixture.sh");
                    let pid_file = root.join("pid");
                    std::fs::write(
                        &script,
                        format!(
                            "#!/bin/sh\nsleep 60 &\necho $! > '{}'\nexit 0\n",
                            pid_file.display()
                        ),
                    )
                    .unwrap();
                    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
                        .unwrap();
                    let task = tokio::spawn(async move {
                        run_control_query(
                            script.to_str().unwrap(),
                            None,
                            b"{}\n",
                            "default",
                            if cancel {
                                Duration::from_secs(10)
                            } else {
                                Duration::from_millis(250)
                            },
                        )
                        .await
                    });
                    tokio::time::timeout(Duration::from_secs(2), async {
                        while !pid_file.exists() {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    })
                    .await
                    .unwrap();
                    let pid: u32 = std::fs::read_to_string(&pid_file)
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if cancel {
                        task.abort();
                        assert!(task.await.unwrap_err().is_cancelled());
                    } else {
                        assert!(task.await.unwrap().unwrap_err().contains("timed out"));
                    }
                    tokio::time::timeout(Duration::from_secs(2), async {
                        while process_running(pid) {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    })
                    .await
                    .expect("query descendant survived cleanup");
                    std::fs::remove_dir_all(root).unwrap();
                }
            });
    }
}
