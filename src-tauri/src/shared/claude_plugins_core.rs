//! Claude Code plugin management through its supported, non-interactive CLI.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::Mutex;

use crate::shared::process_core::{
    configure_owned_process, kill_child_process_tree, tokio_command,
};
use crate::shared::provider_setup_core::{resolve_provider_bin, SetupProvider};
use crate::types::WorkspaceEntry;

const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_PLUGINS: usize = 1000;
const LIST_TIMEOUT: Duration = Duration::from_secs(15);
const ACTION_TIMEOUT: Duration = Duration::from_secs(60);
static PLUGIN_OPERATION: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudePlugin {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) marketplace: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) scope: String,
    pub(crate) enabled: Option<bool>,
    pub(crate) project_path: Option<String>,
    pub(crate) can_modify: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ClaudePluginsResult {
    pub(crate) plugins: Vec<ClaudePlugin>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PluginScope {
    User,
    Project,
    Local,
}

impl PluginScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Local => "local",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PluginAction {
    Enable,
    Disable,
    Install,
    Uninstall,
}

impl PluginAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Install => "install",
            Self::Uninstall => "uninstall",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PluginActionResult {
    pub(crate) message: String,
}

fn valid_plugin_id(plugin: &str) -> bool {
    if plugin.len() > 256 {
        return false;
    }
    let mut parts = plugin.split('@');
    let valid_part = |part: &str| {
        part.as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    match (parts.next(), parts.next(), parts.next()) {
        (Some(name), Some(marketplace), None) => valid_part(name) && valid_part(marketplace),
        _ => false,
    }
}

fn action_args(
    action: PluginAction,
    plugin: &str,
    scope: PluginScope,
) -> Result<Vec<String>, String> {
    if !valid_plugin_id(plugin) {
        return Err("Use a plugin ID in name@marketplace form. URLs, paths, and command options are not plugin IDs.".into());
    }
    if matches!(
        plugin.split_once('@').map(|(_, marketplace)| marketplace),
        Some("inline" | "skills-dir" | "synced")
    ) {
        return Err(
            "This plugin source is managed by Claude Code. Manage it in your Claude terminal."
                .into(),
        );
    }
    let mut args = vec![
        "plugin".into(),
        action.as_str().into(),
        plugin.into(),
        "--scope".into(),
        scope.as_str().into(),
        "--json".into(),
    ];
    if action == PluginAction::Uninstall {
        args.push("--keep-data".into());
    }
    // Never accept a marketplace's arbitrary install command automatically.
    // The CLI refuses such installs without --yes or --accept-command.
    Ok(args)
}

pub(crate) async fn workspace_directory(
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: Option<&str>,
) -> Result<PathBuf, String> {
    let path = match workspace_id {
        Some(id) => workspaces
            .lock()
            .await
            .get(id)
            .map(|workspace| PathBuf::from(&workspace.path))
            .ok_or("Workspace not found. Select an existing workspace and try again.")?,
        None => dirs::home_dir()
            .ok_or("Cannot locate your home directory for Claude plugin management")?,
    };
    let path = path
        .canonicalize()
        .map_err(|error| format!("Cannot access the Claude plugin working directory: {error}"))?;
    if !path.is_dir() {
        return Err("Claude plugin working directory is not a folder".into());
    }
    Ok(path)
}

fn parse_inventory(bytes: &[u8]) -> Result<ClaudePluginsResult, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| "Claude returned invalid plugin JSON. Update Claude Code and retry.")?;
    let entries = value.as_array().ok_or(
        "Claude returned an unsupported plugin inventory format. Update Claude Code and retry.",
    )?;
    if entries.len() > MAX_PLUGINS {
        return Err("Claude plugin inventory exceeds 1000 entries".into());
    }
    let mut plugins = Vec::with_capacity(entries.len());
    for entry in entries {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .ok_or("Claude plugin inventory contains an invalid plugin identity")?;
        let (name, marketplace) = id
            .split_once('@')
            .map(|(name, marketplace)| (name, Some(marketplace.to_string())))
            .unwrap_or((id, None));
        let scope = entry
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let can_modify = valid_plugin_id(id)
            && matches!(scope, "user" | "project" | "local")
            && !matches!(
                marketplace.as_deref(),
                Some("inline" | "skills-dir" | "synced")
            );
        // Do not forward raw entries: mcpServers and other config fields can
        // contain credentials. Keep this explicit display-field allowlist.
        plugins.push(ClaudePlugin {
            id: id.into(),
            name: name.into(),
            marketplace,
            version: entry
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_string),
            scope: scope.into(),
            enabled: entry.get("enabled").and_then(Value::as_bool),
            project_path: entry
                .get("projectPath")
                .and_then(Value::as_str)
                .map(str::to_string),
            can_modify,
        });
    }
    Ok(ClaudePluginsResult { plugins })
}

fn apply_workspace_scope(inventory: &mut ClaudePluginsResult, cwd: &Path) {
    for plugin in &mut inventory.plugins {
        if matches!(plugin.scope.as_str(), "project" | "local") {
            let belongs_to_workspace = plugin.project_path.as_deref().is_some_and(|project| {
                let project = Path::new(project);
                project == cwd || project.canonicalize().is_ok_and(|project| project == cwd)
            });
            plugin.can_modify &= belongs_to_workspace;
        }
    }
}

fn can_apply_action(
    entry: &ClaudePlugin,
    action: PluginAction,
    plugin: &str,
    scope: PluginScope,
) -> bool {
    entry.id == plugin
        && entry.can_modify
        // Enabling/disabling may deliberately create a project/local override
        // for a user installation. Uninstall must target an actual installation.
        && (action != PluginAction::Uninstall || entry.scope == scope.as_str())
}

struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
}

// The child is in its own process group. Also stop descendants if this future
// is canceled while a download or dependency process is still running.
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

async fn read_bounded(
    mut reader: impl AsyncRead + Unpin,
    used: Arc<AtomicUsize>,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|error| format!("Cannot read Claude plugin command output: {error}"))?;
        if count == 0 {
            return Ok(output);
        }
        if used.fetch_add(count, Ordering::Relaxed) + count > MAX_OUTPUT_BYTES {
            return Err("Claude plugin command exceeded the 1 MiB output limit".into());
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

async fn run_cli(
    binary: &Path,
    cwd: &Path,
    args: &[String],
    limit: Duration,
) -> Result<CommandOutput, String> {
    let mut command = tokio_command(binary);
    configure_owned_process(&mut command);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| format!("Cannot start Claude plugin command: {error}. Install Claude Code in provider setup and try again."))?;
    let mut group = ProcessGroupGuard(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or("Claude plugin stdout is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Claude plugin stderr is unavailable")?;
    let used = Arc::new(AtomicUsize::new(0));
    let result = tokio::time::timeout(limit, async {
        let (stdout, _stderr) = futures_util::future::try_join(
            read_bounded(stdout, Arc::clone(&used)),
            read_bounded(stderr, used),
        )
        .await?;
        let status = child
            .wait()
            .await
            .map_err(|error| format!("Cannot wait for Claude plugin command: {error}"))?;
        Ok::<_, String>((stdout, status))
    })
    .await;
    match result {
        Ok(Ok((stdout, status))) => {
            // A reaped PID can be reused; do not signal it after completion.
            group.0 = None;
            Ok(CommandOutput { status, stdout })
        }
        result => {
            kill_child_process_tree(&mut child).await;
            group.0 = None;
            match result {
                Ok(Err(error)) => Err(error),
                _ => Err(format!("Claude plugin command timed out after {} seconds and was stopped. Refresh the plugin list before retrying.", limit.as_secs())),
            }
        }
    }
}

async fn list_at(binary: &Path, cwd: &Path) -> Result<ClaudePluginsResult, String> {
    let output = run_cli(
        binary,
        cwd,
        &["plugin".into(), "list".into(), "--json".into()],
        LIST_TIMEOUT,
    )
    .await?;
    if !output.status.success() {
        return Err(format!("Claude could not list plugins ({}). Run claude plugin list --json in this workspace to inspect its diagnostics.", output.status));
    }
    let mut inventory = parse_inventory(&output.stdout)?;
    apply_workspace_scope(&mut inventory, cwd);
    Ok(inventory)
}

pub(crate) async fn claude_plugins_list_core(
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: Option<String>,
) -> Result<ClaudePluginsResult, String> {
    let _operation = PLUGIN_OPERATION.try_lock().map_err(|_| {
        "Another Claude plugin command is running. Wait for it to finish and retry."
    })?;
    let cwd = workspace_directory(workspaces, workspace_id.as_deref()).await?;
    let binary = resolve_provider_bin(SetupProvider::Claude)
        .await
        .ok_or("Claude Code is not installed. Open provider setup to install it.")?;
    list_at(&binary, &cwd).await
}

fn action_result(
    action: PluginAction,
    plugin: &str,
    scope: PluginScope,
    output: CommandOutput,
) -> Result<PluginActionResult, String> {
    // Official CLI contract: the result is the last stdout line. Earlier lines
    // can contain a marketplace command shown for confirmation; never relay it.
    let text = std::str::from_utf8(&output.stdout).map_err(|_| {
        "Claude plugin result is not valid UTF-8. Refresh the inventory before retrying."
    })?;
    let result: Value = text.lines().rev().find(|line| !line.trim().is_empty())
        .and_then(|line| serde_json::from_str(line).ok())
        .ok_or("Claude did not return a plugin result. Use Claude Code 2.1.268 or newer; refresh the inventory before retrying.")?;
    if result.get("command").and_then(Value::as_str) != Some(action.as_str()) {
        return Err(
            "Claude returned an unexpected plugin result. Refresh the inventory before retrying."
                .into(),
        );
    }
    let already = matches!(action, PluginAction::Enable | PluginAction::Disable)
        && result.get("alreadyInGoalState").and_then(Value::as_bool) == Some(true)
        && result.get("failureCode").and_then(Value::as_str) == Some("already_in_goal_state");
    if (output.status.success() && result.get("outcome").and_then(Value::as_str) == Some("ok"))
        || already
    {
        let verb = match action {
            PluginAction::Enable => "enabled",
            PluginAction::Disable => "disabled",
            PluginAction::Install => "installed",
            PluginAction::Uninstall => "uninstalled",
        };
        return Ok(PluginActionResult {
            message: format!(
                "{plugin} is {verb} at {} scope. Changes apply to the next Claude turn.",
                scope.as_str()
            ),
        });
    }
    if result.get("shownCommand").is_some() {
        return Err("This plugin requires approval for a marketplace-provided install command. Review and install it in your Claude terminal; Hopper did not approve that command.".into());
    }
    let keep_data = if action == PluginAction::Uninstall {
        " --keep-data"
    } else {
        ""
    };
    Err(format!("Claude could not {} {plugin} at {} scope. Run claude plugin {} {plugin} --scope {}{keep_data} in this workspace to inspect its diagnostics, then refresh the list.", action.as_str(), scope.as_str(), action.as_str(), scope.as_str()))
}

pub(crate) async fn claude_plugin_action_core(
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: Option<String>,
    action: PluginAction,
    plugin: String,
    scope: PluginScope,
) -> Result<PluginActionResult, String> {
    let args = action_args(action, &plugin, scope)?;
    if scope != PluginScope::User && workspace_id.is_none() {
        return Err("Select a workspace before changing project or local plugins".into());
    }
    let _operation = PLUGIN_OPERATION.try_lock().map_err(|_| {
        "Another Claude plugin command is running. Wait for it to finish and retry."
    })?;
    let cwd = workspace_directory(workspaces, workspace_id.as_deref()).await?;
    let binary = resolve_provider_bin(SetupProvider::Claude)
        .await
        .ok_or("Claude Code is not installed. Open provider setup to install it.")?;
    if action != PluginAction::Install {
        let inventory = list_at(&binary, &cwd).await?;
        if !inventory
            .plugins
            .iter()
            .any(|entry| can_apply_action(entry, action, &plugin, scope))
        {
            return Err("This plugin is not an installed, editable marketplace plugin in this context. Refresh the list or manage it in Claude Code.".into());
        }
    }
    let output = run_cli(&binary, &cwd, &args, ACTION_TIMEOUT).await?;
    action_result(action, &plugin, scope, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inventory_reports_cli_state_and_does_not_forward_plugin_configuration() {
        let inventory = parse_inventory(&serde_json::to_vec(&json!([
            {"id":"figma@official","version":"2.1","scope":"user","enabled":false,"mcpServers":{"server":{"headers":{"Authorization":"secret"}}}},
            {"id":"tools@market","scope":"project","projectPath":"/project","enabled":true},
            {"id":"locked@market","scope":"managed","enabled":true},
            {"id":"skill@skills-dir","scope":"user"}
        ])).unwrap()).unwrap();
        assert_eq!(inventory.plugins[0].enabled, Some(false));
        assert_eq!(
            inventory.plugins[1].project_path.as_deref(),
            Some("/project")
        );
        assert!(inventory.plugins[0].can_modify);
        assert!(!inventory.plugins[2].can_modify);
        assert!(!inventory.plugins[3].can_modify);
        assert_eq!(inventory.plugins[3].enabled, None);
        let serialized = serde_json::to_string(&inventory).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("mcpServers"));
    }

    #[test]
    fn scoped_installations_cannot_be_changed_from_another_workspace() {
        let mut inventory = parse_inventory(&serde_json::to_vec(&json!([
            {"id":"tools@market","scope":"user","enabled":true,"projectEnabled":false},
            {"id":"tools@market","scope":"project","projectPath":"/other-project","enabled":true},
            {"id":"tools@market","scope":"local","projectPath":"/current-project","enabled":true},
            {"id":"missing@market","scope":"project","enabled":true}
        ])).unwrap()).unwrap();
        apply_workspace_scope(&mut inventory, Path::new("/current-project"));
        assert!(inventory.plugins[0].can_modify);
        assert_eq!(inventory.plugins[0].enabled, Some(true));
        assert!(!inventory.plugins[1].can_modify);
        assert!(inventory.plugins[2].can_modify);
        assert!(!inventory.plugins[3].can_modify);
        assert!(!inventory.plugins.iter().any(|entry| can_apply_action(
            entry,
            PluginAction::Uninstall,
            "tools@market",
            PluginScope::Project
        )));
        assert!(can_apply_action(
            &inventory.plugins[2],
            PluginAction::Uninstall,
            "tools@market",
            PluginScope::Local
        ));
        assert!(can_apply_action(
            &inventory.plugins[0],
            PluginAction::Disable,
            "tools@market",
            PluginScope::Project
        ));
        assert!(!can_apply_action(
            &inventory.plugins[0],
            PluginAction::Uninstall,
            "tools@market",
            PluginScope::Local
        ));
    }

    #[test]
    fn rejects_invalid_inventory_instead_of_claiming_nothing_is_installed() {
        for data in [b"not JSON".as_slice(), b"{}", b"[{\"version\":\"1\"}]"] {
            assert!(parse_inventory(data).is_err());
        }
        assert!(parse_inventory(b"[]").unwrap().plugins.is_empty());
    }

    #[test]
    fn mutation_arguments_have_explicit_scope_and_preserve_data_without_auto_approval() {
        let args = action_args(PluginAction::Uninstall, "safe@market", PluginScope::Local).unwrap();
        assert_eq!(
            args,
            [
                "plugin",
                "uninstall",
                "safe@market",
                "--scope",
                "local",
                "--json",
                "--keep-data"
            ]
        );
        let install = action_args(PluginAction::Install, "safe@market", PluginScope::User).unwrap();
        assert!(!install
            .iter()
            .any(|arg| matches!(arg.as_str(), "--yes" | "-y" | "--accept-command")));
        for invalid in [
            "--all",
            "-x@market",
            "safe",
            "safe@../market",
            "safe@market extra",
            "safe@market;echo",
            "$(id)@market",
            "https://host/plugin",
            "safe@@market",
            "safe@inline",
            "safe@skills-dir",
            "safe@synced",
        ] {
            assert!(
                action_args(PluginAction::Install, invalid, PluginScope::User).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn invalid_workspace_or_scope_is_rejected_before_cli_execution() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let workspaces = Mutex::new(HashMap::new());
                assert!(workspace_directory(&workspaces, Some("missing"))
                    .await
                    .unwrap_err()
                    .contains("Workspace not found"));
                assert!(claude_plugin_action_core(
                    &workspaces,
                    None,
                    PluginAction::Install,
                    "safe@market".into(),
                    PluginScope::Project
                )
                .await
                .unwrap_err()
                .contains("Select a workspace"));
            });
    }

    #[cfg(unix)]
    fn output(code: i32, json: &str) -> CommandOutput {
        use std::os::unix::process::ExitStatusExt;
        CommandOutput {
            status: ExitStatus::from_raw(code << 8),
            stdout: json.as_bytes().to_vec(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn action_results_handle_idempotence_and_do_not_echo_commands_or_credentials() {
        let success = output(
            0,
            "progress\n{\"command\":\"install\",\"outcome\":\"ok\",\"message\":\"secret\"}\n",
        );
        let result = action_result(
            PluginAction::Install,
            "safe@market",
            PluginScope::User,
            success,
        )
        .unwrap();
        assert!(!result.message.contains("secret"));
        let already = output(
            1,
            r#"{"command":"enable","outcome":"failed","failureCode":"already_in_goal_state","alreadyInGoalState":true}"#,
        );
        assert!(action_result(
            PluginAction::Enable,
            "safe@market",
            PluginScope::User,
            already
        )
        .is_ok());
        let refused = output(
            1,
            r#"{"command":"install","outcome":"failed","shownCommand":{"command":"secret"}}"#,
        );
        let error = action_result(
            PluginAction::Install,
            "safe@market",
            PluginScope::User,
            refused,
        )
        .unwrap_err();
        assert!(error.contains("did not approve"));
        assert!(!error.contains("secret"));
        let uninstall_failed = output(
            1,
            r#"{"command":"uninstall","outcome":"failed","message":"secret"}"#,
        );
        let error = action_result(
            PluginAction::Uninstall,
            "safe@market",
            PluginScope::User,
            uninstall_failed,
        )
        .unwrap_err();
        assert!(error.contains("--keep-data"));
        assert!(!error.contains("secret"));
    }

    #[cfg(unix)]
    #[test]
    fn commands_have_bounded_output_and_timeout_without_running_the_user_cli() {
        use std::os::unix::fs::PermissionsExt;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let dir = std::env::temp_dir()
                    .join(format!("hopper-plugin-test-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir(&dir).unwrap();
                let script = dir.join("fake-claude");
                std::fs::write(&script, "#!/bin/sh\nprintf '[]'\nprintf 'ignored' >&2\n").unwrap();
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
                let result = run_cli(&script, &dir, &[], Duration::from_secs(2))
                    .await
                    .unwrap();
                assert_eq!(result.stdout, b"[]");
                std::fs::write(&script, "#!/bin/sh\nhead -c 1048577 /dev/zero\n").unwrap();
                assert!(run_cli(&script, &dir, &[], Duration::from_secs(2))
                    .await
                    .err()
                    .unwrap()
                    .contains("output limit"));
                std::fs::write(&script, "#!/bin/sh\nsleep 60 &\nwait\n").unwrap();
                assert!(run_cli(&script, &dir, &[], Duration::from_millis(30))
                    .await
                    .err()
                    .unwrap()
                    .contains("timed out"));
                std::fs::remove_dir_all(dir).unwrap();
            });
    }
}
