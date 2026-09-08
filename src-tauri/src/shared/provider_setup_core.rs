//! Provider installation and readiness on the machine that runs the agents.
use crate::shared::process_core::tokio_command;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::sync::Mutex;

static SETUP_OPERATION: Mutex<()> = Mutex::const_new(());

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SetupProvider {
    Claude,
    Antigravity,
}
impl SetupProvider {
    pub(crate) fn bin(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Antigravity => "agy",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Antigravity => "Antigravity",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct SetupPreferences {
    pub completed: bool,
    pub claude_enabled: bool,
    pub antigravity_enabled: bool,
    pub antigravity_auto_approve: bool,
}

fn setup_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|p| p.join("hopper"))
        .ok_or("Cannot locate your configuration folder".into())
}

pub(crate) fn read_preferences() -> Result<SetupPreferences, String> {
    read_preferences_at(&setup_dir()?.join("provider-setup.json"))
}
fn read_preferences_at(path: &Path) -> Result<SetupPreferences, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "Provider setup settings contain invalid JSON. Restore the backup or repair the file before continuing.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SetupPreferences::default()),
        Err(e) => Err(format!("Cannot read provider setup settings: {e}")),
    }
}

pub(crate) async fn save_preferences(preferences: SetupPreferences) -> Result<Value, String> {
    let _guard = SETUP_OPERATION
        .try_lock()
        .map_err(|_| "Another setup action is still running")?;
    let dir = setup_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    save_preferences_at(&dir.join("provider-setup.json"), &preferences)?;
    Ok(json!({"ok": true}))
}
fn save_preferences_at(path: &Path, preferences: &SetupPreferences) -> Result<(), String> {
    // Read first, including unknown keys. Never replace malformed user data.
    let mut value = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .map_err(|_| "Invalid provider setup JSON; no changes made".to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e.to_string()),
    };
    let object = value
        .as_object_mut()
        .ok_or("Provider setup settings must be an object")?;
    object.extend(
        serde_json::to_value(preferences)
            .unwrap()
            .as_object()
            .unwrap()
            .clone(),
    );
    if path.exists() {
        std::fs::copy(path, path.with_extension("json.bak")).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(&value).unwrap())
        .map_err(|e| e.to_string())?;
    // Windows rename cannot replace an existing file. The backup remains available.
    #[cfg(windows)]
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

fn executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return path
            .metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub(crate) async fn resolve_provider_bin(provider: SetupProvider) -> Option<PathBuf> {
    let name = provider.bin();
    let mut roots: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = dirs::home_dir() {
        for suffix in [
            ".local/bin",
            ".npm/bin",
            ".yarn/bin",
            "AppData/Local/agy/bin",
            "AppData/Roaming/npm",
        ] {
            roots.push(home.join(suffix));
        }
    }
    roots.extend([
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/bin"),
    ]);
    for root in roots {
        #[cfg(windows)]
        let names = [
            format!("{name}.exe"),
            format!("{name}.cmd"),
            format!("{name}.bat"),
        ];
        #[cfg(not(windows))]
        let names = [name.to_string()];
        for candidate in names {
            let path = root.join(candidate);
            if executable(&path) {
                return Some(path);
            }
        }
    }
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut command = tokio_command(shell);
        // Provider names are enum constants, never caller-supplied shell text.
        command.args(["-lc", &format!("command -v {name}")]);
        if let Ok(output) = checked_output(command, 8).await {
            let path = PathBuf::from(output.trim());
            if executable(&path) {
                return Some(path);
            }
        }
    }
    None
}

async fn checked_output(
    mut command: tokio::process::Command,
    seconds: u64,
) -> Result<String, String> {
    command.stdin(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(seconds), command.output())
        .await
        .map_err(|_| "The command timed out. Check your network and try again.".to_string())?
        .map_err(|e| format!("Could not start command: {e}"))?;
    if !output.status.success() {
        // Provider stderr can contain credentials or account details; keep it local.
        return Err(format!(
            "Command exited with {}. Open the provider terminal to see its diagnostics.",
            output.status
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) async fn setup_status() -> Result<Value, String> {
    let preferences = read_preferences()?;
    let mut providers = Vec::new();
    for provider in [SetupProvider::Claude, SetupProvider::Antigravity] {
        let path = resolve_provider_bin(provider).await;
        let mut version = None;
        let mut authenticated = None;
        if let Some(ref path) = path {
            let mut cmd = tokio_command(path);
            cmd.arg("--version");
            if let Ok(output) = checked_output(cmd, 8).await {
                version = output
                    .lines()
                    .next()
                    .map(|v| v.chars().take(120).collect::<String>());
            }
            if provider == SetupProvider::Claude {
                let mut cmd = tokio_command(path);
                cmd.args(["auth", "status", "--json"]);
                if let Ok(output) = checked_output(cmd, 8).await {
                    authenticated = serde_json::from_str::<Value>(&output)
                        .ok()
                        .and_then(|v| v.get("loggedIn").and_then(Value::as_bool));
                }
            }
        }
        providers.push(json!({"id": provider, "label": provider.label(), "installed": path.is_some(), "path": path, "version": version, "authenticated": authenticated}));
    }
    Ok(
        json!({"preferences": preferences, "providers": providers, "platform": std::env::consts::OS,
        "supported": !cfg!(any(target_os = "ios", target_os = "android"))}),
    )
}

fn installer_url(provider: SetupProvider, windows: bool) -> &'static str {
    match (provider, windows) {
        (SetupProvider::Claude, false) => "https://claude.ai/install.sh",
        (SetupProvider::Claude, true) => "https://claude.ai/install.ps1",
        (SetupProvider::Antigravity, false) => "https://antigravity.google/cli/install.sh",
        (SetupProvider::Antigravity, true) => "https://antigravity.google/cli/install.ps1",
    }
}

pub(crate) async fn setup_action(
    provider: SetupProvider,
    action: &str,
    remote: bool,
) -> Result<Value, String> {
    if cfg!(any(target_os = "ios", target_os = "android")) {
        return Err("Set up providers on your desktop or remote host.".into());
    }
    let _guard = SETUP_OPERATION
        .try_lock()
        .map_err(|_| "Another setup action is still running")?;
    match action {
        "install" => install(provider).await,
        "login" if remote => Err(format!("Sign in on the remote host by running {} in its terminal, then return here and test the connection.", provider.bin())),
        "login" => open_login(provider).await,
        "verify" => verify(provider).await,
        _ => Err("Unknown provider setup action".into()),
    }
}

async fn install(provider: SetupProvider) -> Result<Value, String> {
    if resolve_provider_bin(provider).await.is_some() {
        return Ok(json!({"message": "Already installed. Use the provider CLI to update it."}));
    }
    let url = installer_url(provider, cfg!(windows));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "Could not download the official installer. Check your internet connection.")?
        .error_for_status()
        .map_err(|_| "The official installer is unavailable. Please try again later.")?;
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "Could not read installer")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Installer was unexpectedly large".into());
    }
    let dir = setup_dir()?.join("installers");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!(
        "{}-{}.{}",
        provider.bin(),
        uuid::Uuid::new_v4(),
        if cfg!(windows) { "ps1" } else { "sh" }
    ));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let mut command = tokio_command("powershell.exe");
    #[cfg(windows)]
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&path);
    #[cfg(not(windows))]
    let mut command = tokio_command("bash");
    #[cfg(not(windows))]
    command.arg(&path);
    if provider == SetupProvider::Antigravity {
        command.arg("--skip-aliases");
    }
    let result = checked_output(command, 240).await;
    let _ = std::fs::remove_file(&path);
    result?;
    if resolve_provider_bin(provider).await.is_none() {
        return Err("Installer finished, but the CLI was not found. Restart Hopper or check the provider's installation instructions.".into());
    }
    Ok(json!({"message": "Installed. Sign in, then test the connection."}))
}

fn quote_sh(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
#[cfg(windows)]
fn quote_ps(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

async fn open_login(provider: SetupProvider) -> Result<Value, String> {
    let path = resolve_provider_bin(provider)
        .await
        .ok_or("Install this provider first")?;
    let args = if provider == SetupProvider::Claude {
        " auth login"
    } else {
        ""
    };
    #[cfg(target_os = "macos")]
    {
        let script = format!("{}{}", quote_sh(&path.to_string_lossy()), args);
        let escaped = script.replace('\\', "\\\\").replace('"', "\\\"");
        let mut cmd = tokio_command("/usr/bin/osascript");
        cmd.args([
            "-e",
            &format!("tell application \"Terminal\"\nactivate\ndo script \"{escaped}\"\nend tell"),
        ]);
        checked_output(cmd, 10).await?;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("powershell.exe");
        cmd.creation_flags(0x00000010).args([
            "-NoProfile",
            "-NoExit",
            "-Command",
            &format!("& {}{args}", quote_ps(&path.to_string_lossy())),
        ]);
        cmd.spawn()
            .map_err(|e| format!("Could not open sign-in terminal: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Err(format!("No graphical session found. Run {}{} in your terminal, then test the connection here.", provider.bin(), args));
        }
        let script = format!("{}{}; printf '\\nReturn to Hopper and click Test connection. Press Enter to close.\\n'; read -r hopper_answer", quote_sh(&path.to_string_lossy()), args);
        let mut launched = false;
        for (terminal, flag) in [
            ("x-terminal-emulator", "-e"),
            ("gnome-terminal", "--"),
            ("konsole", "-e"),
            ("kitty", "--"),
            ("alacritty", "-e"),
            ("xterm", "-e"),
        ] {
            let mut cmd = tokio_command(terminal);
            cmd.args([flag, "sh", "-c", &script]);
            if let Ok(mut child) = cmd.spawn() {
                tokio::time::sleep(Duration::from_millis(150)).await;
                if matches!(child.try_wait(), Ok(Some(status)) if !status.success()) {
                    continue;
                }
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
                launched = true;
                break;
            }
        }
        if !launched {
            return Err(format!("No supported terminal found. Run {}{} in your terminal, then test the connection here.", provider.bin(), args));
        }
    }
    Ok(json!({"message": "Complete sign-in in the terminal/browser, then click Test connection."}))
}

async fn verify(provider: SetupProvider) -> Result<Value, String> {
    let path = resolve_provider_bin(provider)
        .await
        .ok_or("Install this provider first")?;
    // Antigravity's print prompt consumes model quota, so use its account-safe
    // model discovery command for the connection check instead.
    if provider == SetupProvider::Antigravity {
        let mut command = tokio_command(path);
        command.arg("models");
        checked_output(command, 30).await.map_err(|e| {
            format!("Connection check failed. Sign in and check account access. {e}")
        })?;
        return Ok(json!({"verified": true, "message": "Connection verified. Ready to use in Hopper."}));
    }
    // An isolated empty directory prevents this check from reading a user's project.
    let dir = std::env::temp_dir().join(format!("hopper-provider-check-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let mut command = tokio_command(path);
    command.current_dir(&dir);
    if provider == SetupProvider::Claude {
        command.args([
            "-p",
            "Reply with exactly HOPPER_READY. Do not use tools.",
            "--output-format",
            "json",
            "--tools",
            "",
            "--no-session-persistence",
        ]);
    } else {
        command.args([
            "-p",
            "Reply with exactly HOPPER_READY. Do not use tools.",
            "--output-format",
            "json",
            "--mode",
            "plan",
            "--print-timeout",
            "60s",
        ]);
    }
    let result = checked_output(command, 75).await;
    let _ = std::fs::remove_dir_all(&dir);
    let output = result
        .map_err(|e| format!("Connection check failed. Sign in and check account access. {e}"))?;
    let value: Value = serde_json::from_str(output.trim()).map_err(|_| {
        "The CLI returned an unsupported response. Update it using its official installer."
    })?;
    let success = verification_succeeded(provider, &value);
    if !success {
        return Err("The provider did not complete the test. Check its account, model access, and configuration in the provider terminal.".into());
    }
    Ok(json!({"verified": true, "message": "Connection verified. Ready to use in Hopper."}))
}

fn verification_succeeded(provider: SetupProvider, value: &Value) -> bool {
    match provider {
        SetupProvider::Claude => {
            value.get("is_error").and_then(Value::as_bool) == Some(false)
                && value
                    .get("result")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.contains("HOPPER_READY"))
        }
        SetupProvider::Antigravity => {
            value.get("status").and_then(Value::as_str) == Some("SUCCESS")
                && value
                    .get("response")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.contains("HOPPER_READY"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verification_requires_a_successful_provider_response() {
        assert!(verification_succeeded(
            SetupProvider::Claude,
            &json!({"is_error": false, "result": "HOPPER_READY"})
        ));
        assert!(!verification_succeeded(
            SetupProvider::Claude,
            &json!({"is_error": true, "result": "HOPPER_READY"})
        ));
        assert!(verification_succeeded(
            SetupProvider::Antigravity,
            &json!({"status": "SUCCESS", "response": "HOPPER_READY"})
        ));
        assert!(!verification_succeeded(
            SetupProvider::Antigravity,
            &json!({"status": "WAITING", "response": "HOPPER_READY"})
        ));
        assert!(!verification_succeeded(
            SetupProvider::Antigravity,
            &json!({"status": "SUCCESS", "response": "Please sign in"})
        ));
    }
    #[test]
    fn setup_preserves_unknown_settings_and_backs_up() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("provider-setup.json");
        let original = r#"{"custom":42,"antigravityAutoApprove":false}"#;
        std::fs::write(&path, original).unwrap();
        save_preferences_at(
            &path,
            &SetupPreferences {
                completed: true,
                ..Default::default()
            },
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["custom"], 42);
        assert_eq!(
            std::fs::read_to_string(path.with_extension("json.bak")).unwrap(),
            original
        );
        assert!(!read_preferences_at(&path).unwrap().antigravity_auto_approve);
        std::fs::write(&path, "invalid").unwrap();
        assert!(save_preferences_at(&path, &SetupPreferences::default()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn hook_is_session_scoped_and_encodes_workspace_identity() {
        let value = claude_hook_settings(1234, "token", "work space&other");
        let hook = &value["hooks"]["PreToolUse"][0]["hooks"][0];
        assert_eq!(hook["type"], "http");
        assert_eq!(hook["headers"]["X-Hopper-Token"], "token");
        let url = reqwest::Url::parse(hook["url"].as_str().unwrap()).unwrap();
        assert_eq!(url.query_pairs().next().unwrap().1, "work space&other");
        assert_eq!(
            claude_permission_response(false)["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert_eq!(
            claude_permission_response(true)["hookSpecificOutput"]["permissionDecision"],
            "allow"
        );
    }
    #[test]
    fn only_known_provider_installers_are_selected() {
        assert!(serde_json::from_str::<SetupProvider>("\"arbitrary-command\"").is_err());
        assert_eq!(
            installer_url(SetupProvider::Claude, false),
            "https://claude.ai/install.sh"
        );
        assert_eq!(quote_sh("a'b $(x)"), "'a'\"'\"'b $(x)'");
    }
}

pub(crate) fn claude_hook_settings(port: u16, token: &str, workspace_id: &str) -> Value {
    let mut url = reqwest::Url::parse(&format!("http://127.0.0.1:{port}/permission")).unwrap();
    url.query_pairs_mut()
        .append_pair("workspace_id", workspace_id);
    json!({"hooks": {"PreToolUse": [{"matcher": ".*", "hooks": [{
        "type": "http", "url": url.as_str(), "timeout": 130,
        "headers": {"X-Hopper-Token": token}
    }]}]}})
}

pub(crate) fn claude_permission_response(approved: bool) -> Value {
    json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": if approved { "allow" } else { "deny" },
        "permissionDecisionReason": if approved { "Approved in Hopper" } else { "Declined or timed out in Hopper" }
    }})
}
