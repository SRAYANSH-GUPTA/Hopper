use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::types::{AppSettings, WorkspaceEntry, WorkspaceSettings};
use serde_json::Value;

fn normalize_windows_namespace_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }

    fn strip_prefix_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
        value
            .get(..prefix.len())
            .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
            .map(|_| &value[prefix.len()..])
    }

    fn starts_with_drive_path(value: &str) -> bool {
        let bytes = value.as_bytes();
        bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/')
    }

    if let Some(rest) = strip_prefix_ascii_case(path, r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = strip_prefix_ascii_case(path, "//?/UNC/") {
        return format!("//{rest}");
    }
    if let Some(rest) =
        strip_prefix_ascii_case(path, r"\\?\").filter(|rest| starts_with_drive_path(rest))
    {
        return rest.to_string();
    }
    if let Some(rest) =
        strip_prefix_ascii_case(path, "//?/").filter(|rest| starts_with_drive_path(rest))
    {
        return rest.to_string();
    }
    if let Some(rest) =
        strip_prefix_ascii_case(path, r"\\.\").filter(|rest| starts_with_drive_path(rest))
    {
        return rest.to_string();
    }
    if let Some(rest) =
        strip_prefix_ascii_case(path, "//./").filter(|rest| starts_with_drive_path(rest))
    {
        return rest.to_string();
    }

    path.to_string()
}

fn normalize_optional_windows_namespace_path(path: Option<String>) -> (Option<String>, bool) {
    match path {
        Some(path) => {
            let normalized = normalize_windows_namespace_path(&path);
            let changed = normalized != path;
            (Some(normalized), changed)
        }
        None => (None, false),
    }
}

fn normalize_workspace_settings(settings: WorkspaceSettings) -> (WorkspaceSettings, bool) {
    let (worktrees_folder, changed) =
        normalize_optional_windows_namespace_path(settings.worktrees_folder.clone());
    (
        WorkspaceSettings {
            worktrees_folder,
            ..settings
        },
        changed,
    )
}

fn normalize_workspace_entry(entry: WorkspaceEntry) -> (WorkspaceEntry, bool) {
    let normalized_path = normalize_windows_namespace_path(&entry.path);
    let (settings, settings_changed) = normalize_workspace_settings(entry.settings.clone());
    let changed = normalized_path != entry.path || settings_changed;
    (
        WorkspaceEntry {
            path: normalized_path,
            settings,
            ..entry
        },
        changed,
    )
}

fn normalize_workspace_entries<I>(entries: I) -> (Vec<WorkspaceEntry>, bool)
where
    I: IntoIterator<Item = WorkspaceEntry>,
{
    let mut changed = false;
    let normalized = entries
        .into_iter()
        .map(|entry| {
            let (entry, entry_changed) = normalize_workspace_entry(entry);
            changed |= entry_changed;
            entry
        })
        .collect();
    (normalized, changed)
}

fn normalize_app_settings(settings: AppSettings) -> (AppSettings, bool) {
    let (global_worktrees_folder, mut changed) =
        normalize_optional_windows_namespace_path(settings.global_worktrees_folder.clone());
    let antigravity_model_id = settings.antigravity_model_id.as_deref().map(|model| {
        let migrated = match model.trim() {
            "Gemini 3.5 Flash (High)" => "Gemini 3.8 Flash (High)",
            "Gemini 3.5 Flash (Medium)" => "Gemini 3.8 Flash (Medium)",
            "Gemini 3.5 Flash (Low)" => "Gemini 3.8 Flash (Low)",
            current => current,
        };
        if migrated != model {
            changed = true;
        }
        migrated.to_string()
    });
    (
        AppSettings {
            global_worktrees_folder,
            antigravity_model_id,
            ..settings
        },
        changed,
    )
}

fn try_rewrite_settings_with_normalized_paths(path: &PathBuf, settings: &AppSettings) {
    if let Err(error) = write_settings(path, settings) {
        eprintln!(
            "read_settings: failed to persist normalized settings paths to {}: {}",
            path.display(),
            error
        );
    }
}

pub(crate) fn read_workspaces(path: &PathBuf) -> Result<HashMap<String, WorkspaceEntry>, String> {
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(storage_read_error(path, error)),
    };
    let list: Vec<WorkspaceEntry> =
        serde_json::from_str(&data).map_err(|error| storage_read_error(path, error))?;
    let (list, _) = normalize_workspace_entries(list);
    Ok(list
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect())
}

pub(crate) fn write_workspaces(path: &PathBuf, entries: &[WorkspaceEntry]) -> Result<(), String> {
    let (entries, _) = normalize_workspace_entries(entries.iter().cloned());
    let data = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
    write_storage_atomic(path, data.as_bytes()).map_err(|e| e.to_string())
}

pub(crate) fn read_settings(path: &PathBuf) -> Result<AppSettings, String> {
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AppSettings::default())
        }
        Err(error) => return Err(storage_read_error(path, error)),
    };
    let mut value: Value =
        serde_json::from_str(&data).map_err(|error| storage_read_error(path, error))?;
    migrate_follow_up_message_behavior(&mut value);
    match serde_json::from_value(value.clone()) {
        Ok(settings) => Ok(finalize_loaded_settings(path, settings)),
        Err(_) => {
            sanitize_remote_settings_for_tcp_only(&mut value);
            migrate_follow_up_message_behavior(&mut value);
            serde_json::from_value(value)
                .map(|settings| finalize_loaded_settings(path, settings))
                .map_err(|error| storage_read_error(path, error))
        }
    }
}

pub(crate) fn write_settings(path: &PathBuf, settings: &AppSettings) -> Result<(), String> {
    let (settings, _) = normalize_app_settings(settings.clone());
    let data = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    write_storage_atomic(path, data.as_bytes()).map_err(|e| e.to_string())
}

fn storage_read_error(path: &Path, error: impl std::fmt::Display) -> String {
    format!(
        "Cannot load {}: {error}. Repair or restore this file before restarting Hopper; its contents have not been replaced.",
        path.display()
    )
}

// Both the desktop app and daemon use this persistence path. Write and sync a
// private sibling first so a failed write or interrupted process cannot truncate
// the last complete settings/workspace snapshot.
pub(crate) fn write_storage_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".hopper-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Settings include remote authentication tokens.
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| {
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn finalize_loaded_settings(path: &PathBuf, settings: AppSettings) -> AppSettings {
    let (settings, changed) = normalize_app_settings(settings);
    if changed {
        try_rewrite_settings_with_normalized_paths(path, &settings);
    }
    settings
}

fn sanitize_remote_settings_for_tcp_only(value: &mut Value) {
    let Value::Object(root) = value else {
        return;
    };
    root.insert(
        "remoteBackendProvider".to_string(),
        Value::String("tcp".to_string()),
    );
    if let Some(Value::Array(remote_backends)) = root.get_mut("remoteBackends") {
        for entry in remote_backends {
            let Value::Object(entry_obj) = entry else {
                continue;
            };
            entry_obj.insert("provider".to_string(), Value::String("tcp".to_string()));
            entry_obj.retain(|key, _| {
                matches!(
                    key.as_str(),
                    "id" | "name" | "provider" | "host" | "token" | "lastConnectedAtMs"
                )
            });
        }
    }
    root.retain(|key, _| !key.to_ascii_lowercase().starts_with("orb"));
}

fn migrate_follow_up_message_behavior(value: &mut Value) {
    let Value::Object(root) = value else {
        return;
    };
    if root.contains_key("followUpMessageBehavior") {
        return;
    }
    let steer_enabled = root
        .get("steerEnabled")
        .or_else(|| root.get("experimentalSteerEnabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    root.insert(
        "followUpMessageBehavior".to_string(),
        Value::String(if steer_enabled { "steer" } else { "queue" }.to_string()),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        read_settings, read_workspaces, write_settings, write_storage_atomic, write_workspaces,
    };
    use crate::types::{AppSettings, WorkspaceEntry, WorkspaceKind, WorkspaceSettings};
    use uuid::Uuid;

    #[test]
    fn corrupt_storage_returns_a_recovery_error_without_overwriting_data() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        for filename in ["settings.json", "workspaces.json"] {
            let path = temp_dir.join(filename);
            let contents = b"{interrupted JSON";
            std::fs::write(&path, contents).unwrap();
            let error = if filename == "settings.json" {
                read_settings(&path).unwrap_err()
            } else {
                read_workspaces(&path).unwrap_err()
            };
            assert!(error.contains(&path.display().to_string()));
            assert!(error.contains("Repair or restore"));
            assert_eq!(std::fs::read(&path).unwrap(), contents);
        }
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn missing_storage_uses_initial_defaults() {
        let root = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        assert!(read_settings(&root.join("settings.json")).is_ok());
        assert!(read_workspaces(&root.join("workspaces.json"))
            .unwrap()
            .is_empty());
        assert!(!root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_the_previous_snapshot_for_existing_readers() {
        use std::io::Read;
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        let path = root.join("settings.json");
        write_storage_atomic(&path, b"previous complete snapshot").unwrap();
        let mut previous_reader = std::fs::File::open(&path).unwrap();
        write_storage_atomic(&path, b"next snapshot").unwrap();

        let mut previous = String::new();
        previous_reader.read_to_string(&mut previous).unwrap();
        assert_eq!(previous, "previous complete snapshot");
        assert_eq!(std::fs::read(&path).unwrap(), b"next snapshot");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_atomic_replace_preserves_destination_and_cleans_temporary_file() {
        let root = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        let path = root.join("settings.json");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("existing"), b"keep me").unwrap();
        assert!(write_storage_atomic(&path, b"new data").is_err());
        assert_eq!(std::fs::read(path.join("existing")).unwrap(), b"keep me");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn write_read_workspaces_persists_sort_and_group() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("workspaces.json");

        let mut settings = WorkspaceSettings::default();
        settings.sort_order = Some(5);
        settings.group_id = Some("group-42".to_string());
        settings.sidebar_collapsed = true;
        settings.git_root = Some("/tmp".to_string());

        let entry = WorkspaceEntry {
            id: "w1".to_string(),
            name: "Workspace".to_string(),
            path: "/tmp".to_string(),
            kind: WorkspaceKind::Main,
            parent_id: None,
            worktree: None,
            settings: settings.clone(),
        };

        write_workspaces(&path, &[entry]).expect("write workspaces");
        let read = read_workspaces(&path).expect("read workspaces");
        let stored = read.get("w1").expect("stored workspace");
        assert_eq!(stored.settings.sort_order, Some(5));
        assert_eq!(stored.settings.group_id.as_deref(), Some("group-42"));
        assert!(stored.settings.sidebar_collapsed);
        assert_eq!(stored.settings.git_root.as_deref(), Some("/tmp"));
    }

    #[test]
    fn write_read_workspaces_sanitizes_windows_namespace_paths() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("workspaces.json");

        let entry = WorkspaceEntry {
            id: "w1".to_string(),
            name: "Workspace".to_string(),
            path: r"\\?\I:\gpt-projects\json-composer".to_string(),
            kind: WorkspaceKind::Main,
            parent_id: None,
            worktree: None,
            settings: WorkspaceSettings::default(),
        };

        write_workspaces(&path, &[entry]).expect("write workspaces");

        let read = read_workspaces(&path).expect("read workspaces");
        let stored = read.get("w1").expect("stored workspace");
        assert_eq!(stored.path, r"I:\gpt-projects\json-composer");
    }

    #[test]
    fn read_workspaces_sanitizes_namespace_paths_without_rewriting_file() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("workspaces.json");

        std::fs::write(
            &path,
            r#"[
  {
    "id": "w1",
    "name": "Workspace",
    "path": "\\\\?\\I:\\gpt-projects\\json-composer",
    "kind": "main",
    "parentId": null,
    "worktree": null,
    "settings": {}
  }
]"#,
        )
        .expect("write workspaces");

        let read = read_workspaces(&path).expect("read workspaces");
        let stored = read.get("w1").expect("stored workspace");
        assert_eq!(stored.path, r"I:\gpt-projects\json-composer");

        let persisted = std::fs::read_to_string(&path).expect("read persisted workspaces");
        let persisted_entries: Vec<WorkspaceEntry> =
            serde_json::from_str(&persisted).expect("deserialize persisted workspaces");
        assert_eq!(persisted_entries.len(), 1);
        assert_eq!(
            persisted_entries[0].path,
            r"\\?\I:\gpt-projects\json-composer"
        );
    }

    #[test]
    fn read_settings_sanitizes_non_tcp_remote_provider() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("settings.json");

        std::fs::write(
            &path,
            r#"{
  "remoteBackendProvider": "legacy-provider",
  "remoteBackendHost": "example:4732",
  "remoteBackendToken": "token-1",
  "remoteBackends": [
    {
      "id": "remote-a",
      "name": "Remote A",
      "provider": "legacy-provider",
      "host": "example:4732",
      "token": "token-1",
      "legacyWsUrl": "wss://example/ws"
    }
  ],
  "theme": "dark"
}"#,
        )
        .expect("write settings");

        let settings = read_settings(&path).expect("read settings");
        assert!(matches!(
            settings.remote_backend_provider,
            crate::types::RemoteBackendProvider::Tcp
        ));
        assert_eq!(settings.remote_backends.len(), 1);
        assert!(matches!(
            settings.remote_backends[0].provider,
            crate::types::RemoteBackendProvider::Tcp
        ));
        assert_eq!(settings.theme, "dark");
    }

    #[test]
    fn read_settings_migrates_follow_up_behavior_from_legacy_steer_enabled_true() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("settings.json");

        std::fs::write(
            &path,
            r#"{
  "steerEnabled": true,
  "theme": "dark"
}"#,
        )
        .expect("write settings");

        let settings = read_settings(&path).expect("read settings");
        assert!(settings.steer_enabled);
        assert_eq!(settings.follow_up_message_behavior, "steer");
    }

    #[test]
    fn read_settings_migrates_follow_up_behavior_from_legacy_steer_enabled_false() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("settings.json");

        std::fs::write(
            &path,
            r#"{
  "steerEnabled": false,
  "theme": "dark"
}"#,
        )
        .expect("write settings");

        let settings = read_settings(&path).expect("read settings");
        assert!(!settings.steer_enabled);
        assert_eq!(settings.follow_up_message_behavior, "queue");
    }

    #[test]
    fn write_read_settings_sanitizes_global_worktrees_folder_namespace_paths() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("settings.json");

        let mut settings = AppSettings::default();
        settings.global_worktrees_folder = Some(r"\\?\I:\gpt-projects\worktrees".to_string());

        write_settings(&path, &settings).expect("write settings");
        let read = read_settings(&path).expect("read settings");
        assert_eq!(
            read.global_worktrees_folder.as_deref(),
            Some(r"I:\gpt-projects\worktrees")
        );
    }

    #[test]
    fn read_settings_migrates_removed_antigravity_flash_models() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("settings.json");

        let mut settings = AppSettings::default();
        settings.antigravity_model_id = Some("Gemini 3.5 Flash (Low)".to_string());
        write_settings(&path, &settings).expect("write settings");

        let migrated = read_settings(&path).expect("read settings");
        assert_eq!(
            migrated.antigravity_model_id.as_deref(),
            Some("Gemini 3.8 Flash (Low)")
        );

        let persisted = std::fs::read_to_string(&path).expect("read persisted settings");
        assert!(persisted.contains("Gemini 3.8 Flash (Low)"));
        assert!(!persisted.contains("Gemini 3.5 Flash"));
    }

    #[test]
    fn read_settings_rewrites_global_worktrees_folder_namespace_paths() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("settings.json");

        std::fs::write(
            &path,
            r#"{
  "globalWorktreesFolder": "\\\\?\\I:\\gpt-projects\\worktrees",
  "theme": "dark"
}"#,
        )
        .expect("write settings");

        let settings = read_settings(&path).expect("read settings");
        assert_eq!(
            settings.global_worktrees_folder.as_deref(),
            Some(r"I:\gpt-projects\worktrees")
        );

        let rewritten = std::fs::read_to_string(&path).expect("read rewritten settings");
        let rewritten_settings: AppSettings =
            serde_json::from_str(&rewritten).expect("deserialize rewritten settings");
        assert_eq!(
            rewritten_settings.global_worktrees_folder.as_deref(),
            Some(r"I:\gpt-projects\worktrees")
        );
    }

    #[test]
    fn read_settings_keeps_existing_follow_up_behavior() {
        let temp_dir = std::env::temp_dir().join(format!("hopper-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("settings.json");

        std::fs::write(
            &path,
            r#"{
  "steerEnabled": true,
  "followUpMessageBehavior": "queue",
  "theme": "dark"
}"#,
        )
        .expect("write settings");

        let settings = read_settings(&path).expect("read settings");
        assert_eq!(settings.follow_up_message_behavior, "queue");
    }
}
