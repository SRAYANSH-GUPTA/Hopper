use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use regex::Regex;
use tokio::process::Command;

use crate::shared::process_core::tokio_command;

async fn resolve_binary(binary: &str, fallbacks: &[&str]) -> String {
    let out = Command::new("/bin/sh")
        .args(["-lc", &format!("which {binary} 2>/dev/null || command -v {binary} 2>/dev/null")])
        .output()
        .await;

    if let Ok(output) = out {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return path;
        }
    }

    for candidate in fallbacks {
        if std::path::Path::new(candidate).exists() {
            return (*candidate).to_string();
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        for suffix in [".local/bin", ".npm/bin", ".yarn/bin"] {
            let candidate = PathBuf::from(&home).join(suffix).join(binary);
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }

    binary.to_string()
}

async fn run_binary_usage(
    binary: &str,
    fallbacks: &[&str],
    workspace_path: Option<String>,
) -> Result<String, String> {
    let resolved_binary = resolve_binary(binary, fallbacks).await;
    let mut command = tokio_command(&resolved_binary);
    command.arg("/usage");
    if let Some(path) = workspace_path.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        command.current_dir(path);
    }

    let output = command
        .output()
        .await
        .map_err(|err| format!("Failed to run {binary} usage: {err}"))?;

    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let trimmed = text.trim().to_string();
    if !trimmed.is_empty() {
        return Ok(trimmed);
    }

    if output.status.success() {
        return Err(format!("{binary} usage returned no output."));
    }

    Err(format!(
        "{binary} usage exited with status {} and returned no output.",
        output.status
    ))
}

fn strip_ansi_sequences(text: &str) -> String {
    // Keep the captured PTY output readable by removing common ANSI control codes.
    static ANSI_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = ANSI_RE.get_or_init(|| {
        Regex::new(r"\x1B\[[0-9;?]*[ -/]*[@-~]").expect("valid ANSI escape regex")
    });
    re.replace_all(text, "").to_string()
}

fn looks_ready_for_slash_command(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("signed in")
        || lower.contains("sign in")
        || lower.contains("authenticate")
        || lower.contains("authorization")
        || lower.contains("prompt box")
        || lower.contains("type your goal")
        || lower.contains("press enter")
        || lower.contains("slash commands")
        || lower.contains("? for shortcuts")
        || lower.trim_end().ends_with('>')
}

fn has_steady_prompt_marker(text: &str) -> bool {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return false;
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let tail = lines.last().copied().unwrap_or(trimmed);
    let tail = tail.trim_end();
    tail.ends_with('>')
        || tail.ends_with('❯')
        || tail.ends_with('$')
        || tail.ends_with('#')
        || tail.ends_with("] ")
        || tail.ends_with(": ")
        || tail.ends_with(">>")
}

fn looks_like_usage_screen(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("models & quota")
        || lower.contains("weekly limit")
        || lower.contains("five hour limit")
}

fn is_usage_screen_complete(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    looks_like_usage_screen(text) && (lower.contains("esc close") || lower.contains("esc to cancel"))
}

fn usage_trace(message: &str) {
    eprintln!("[provider-usage][antigravity] {message}");
}

fn run_binary_usage_pty_blocking(
    binary: &str,
    resolved_binary: String,
    workspace_path: Option<String>,
) -> Result<String, String> {
    usage_trace(&format!("starting pty usage capture for {binary}"));
    usage_trace(&format!("resolved binary: {resolved_binary}"));
    if let Some(path) = workspace_path.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        usage_trace(&format!("workspace cwd: {path}"));
    } else {
        usage_trace("workspace cwd: <unset>");
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| format!("Failed to open pty for {binary}: {err}"))?;
    usage_trace("opened pty");

    let mut cmd = CommandBuilder::new(&resolved_binary);
    if let Some(path) = workspace_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        cmd.cwd(path);
    }
    cmd.env("TERM", "xterm-256color");
    let locale = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "en_US.UTF-8".to_string());
    cmd.env("LANG", &locale);
    cmd.env("LC_ALL", &locale);
    cmd.env("LC_CTYPE", &locale);

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|err| format!("Failed to spawn {binary} in pty: {err}"))?;
    usage_trace(&format!("spawned {binary} pid={:?}", child.process_id()));
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| format!("Failed to open pty reader for {binary}: {err}"))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|err| format!("Failed to open pty writer for {binary}: {err}"))?;

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let reader_handle = std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if tx.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let startup_timeout = Duration::from_secs(30);
    let startup_quiet_after = Duration::from_millis(1200);
    let startup_started = Instant::now();
    let mut startup_last_output = Instant::now();
    let mut startup_saw_output = false;
    let mut startup_ready_marker_seen = false;
    let mut startup_raw = Vec::new();

    loop {
        let wait = if startup_saw_output {
            startup_quiet_after
        } else {
            Duration::from_millis(250)
        };
        match rx.recv_timeout(wait) {
            Ok(chunk) => {
                startup_saw_output = true;
                startup_last_output = Instant::now();
                let chunk_text = String::from_utf8_lossy(&chunk).to_string();
                startup_raw.extend_from_slice(&chunk);
                let clean_chunk = strip_ansi_sequences(&chunk_text);
                let preview = clean_chunk.trim().chars().take(200).collect::<String>();
                if !preview.is_empty() {
                    usage_trace(&format!("startup chunk: {preview}"));
                }
                if looks_ready_for_slash_command(&clean_chunk) || has_steady_prompt_marker(&clean_chunk) {
                    usage_trace("ready marker detected during startup wait");
                    startup_ready_marker_seen = true;
                }
                let startup_snapshot = strip_ansi_sequences(&String::from_utf8_lossy(&startup_raw));
                if looks_like_usage_screen(&startup_snapshot) {
                    usage_trace("usage screen detected during startup; returning it directly");
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if startup_ready_marker_seen && startup_last_output.elapsed() >= startup_quiet_after {
                    usage_trace("startup settled after ready marker");
                    break;
                }
                if startup_started.elapsed() >= startup_timeout {
                    usage_trace("startup timeout reached before ready marker");
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                usage_trace("startup reader disconnected");
                break;
            }
        }
    }

    let startup_text = strip_ansi_sequences(&String::from_utf8_lossy(&startup_raw))
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string();
    if looks_like_usage_screen(&startup_text) {
        usage_trace("returning startup usage screen without sending /usage");
        let _ = writer.flush();
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader_handle.join();
        return Ok(startup_text);
    }

    for byte in b"/usage " {
        writer
            .write_all(&[*byte])
            .map_err(|err| format!("Failed to write to {binary}: {err}"))?;
        let _ = writer.flush();
        std::thread::sleep(Duration::from_millis(80)); // Type slightly slower
    }
    
    // Wait for the autocomplete menu to close because of the space
    std::thread::sleep(Duration::from_millis(500));
    
    writer
        .write_all(b"\r")
        .map_err(|err| format!("Failed to write \\r to {binary}: {err}"))?;
    let _ = writer.flush();
    usage_trace("wrote /usage and flushed input");

    let mut raw = Vec::new();
    let start = Instant::now();
    let mut saw_output = false;
    let max_wait = Duration::from_secs(12);
    let mut last_output = Instant::now();

    loop {
        let current_text = strip_ansi_sequences(&String::from_utf8_lossy(&raw));
        let is_complete = is_usage_screen_complete(&current_text);
        let actual_settle = if is_complete {
            Duration::from_millis(500)
        } else {
            Duration::from_millis(4000)
        };

        let wait = if saw_output {
            actual_settle
        } else {
            Duration::from_millis(250)
        };
        match rx.recv_timeout(wait) {
            Ok(chunk) => {
                saw_output = true;
                last_output = Instant::now();
                let preview = String::from_utf8_lossy(&chunk)
                    .replace('\r', "\\r")
                    .replace('\n', "\\n")
                    .chars()
                    .take(200)
                    .collect::<String>();
                if !preview.is_empty() {
                    usage_trace(&format!("usage chunk: {preview}"));
                }
                raw.extend_from_slice(&chunk);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if saw_output && last_output.elapsed() >= actual_settle {
                    usage_trace("usage output settled");
                    break;
                }
                if start.elapsed() >= max_wait {
                    usage_trace("usage capture timeout reached");
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                usage_trace("usage reader disconnected");
                break;
            }
        }
    }

    usage_trace("closing pty session");
    let _ = writer.flush();
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader_handle.join();

    let text = String::from_utf8_lossy(&raw).to_string();
    let cleaned = strip_ansi_sequences(&text)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string();
    if !cleaned.is_empty() {
        usage_trace(&format!("returning {} bytes of cleaned output", cleaned.len()));
        Ok(cleaned)
    } else if saw_output || startup_saw_output {
        let startup_preview = strip_ansi_sequences(&String::from_utf8_lossy(&startup_raw))
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim()
            .chars()
            .take(240)
            .collect::<String>();
        if startup_preview.is_empty() {
            usage_trace("captured output was unreadable");
            Err(format!("{binary} usage produced unreadable output."))
        } else {
            usage_trace(&format!("captured unreadable startup preview: {startup_preview}"));
            Err(format!(
                "{binary} usage produced unreadable output after startup text: {startup_preview}"
            ))
        }
    } else {
        usage_trace("captured no output at all");
        Err(format!("{binary} usage returned no output."))
    }
}

pub(crate) async fn provider_usage_output_core(
    provider: &str,
    workspace_path: Option<String>,
) -> Result<String, String> {
    match provider {
        "claude" => {
            run_binary_usage(
                "claude",
                &[
                    "/usr/local/bin/claude",
                    "/opt/homebrew/bin/claude",
                ],
                workspace_path,
            )
            .await
        }
        "antigravity" => {
            let resolved_binary = resolve_binary(
                "agy",
                &[
                    "/usr/local/bin/agy",
                    "/opt/homebrew/bin/agy",
                ],
            )
            .await;
            tokio::task::spawn_blocking(move || {
                run_binary_usage_pty_blocking(
                    "agy",
                    resolved_binary,
                    workspace_path,
                )
            })
            .await
            .map_err(|err| format!("Failed to run agy usage in pty: {err}"))?
        }
        other => Err(format!("Unsupported provider for usage output: {other}")),
    }
}
