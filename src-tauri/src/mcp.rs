use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct McpServerConfig {
    pub command: String,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}

fn claude_settings_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::PathBuf::from(home)
        .join(".claude")
        .join("settings.json")
}

fn read_settings() -> Result<Value, String> {
    let path = claude_settings_path();
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read ~/.claude/settings.json: {e}"))?;
    if content.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse ~/.claude/settings.json: {e}"))
}

fn write_settings(value: &Value) -> Result<(), String> {
    let path = claude_settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create ~/.claude dir: {e}"))?;
    }
    let content = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write ~/.claude/settings.json: {e}"))
}

/// Returns the current mcpServers map from ~/.claude/settings.json.
/// Keys are the server IDs, values are the config objects.
#[tauri::command]
pub fn mcp_list_servers() -> Result<HashMap<String, McpServerConfig>, String> {
    let settings = read_settings()?;
    let servers = settings
        .get("mcpServers")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    serde_json::from_value(servers)
        .map_err(|e| format!("Failed to parse mcpServers: {e}"))
}

/// Adds or replaces an MCP server entry in ~/.claude/settings.json.
#[tauri::command]
pub fn mcp_add_server(id: String, config: McpServerConfig) -> Result<(), String> {
    let mut settings = read_settings()?;
    if settings.get("mcpServers").is_none() {
        settings["mcpServers"] = serde_json::json!({});
    }
    let config_value =
        serde_json::to_value(&config).map_err(|e| format!("Failed to serialize config: {e}"))?;
    settings["mcpServers"][&id] = config_value;
    write_settings(&settings)
}

/// Removes an MCP server entry from ~/.claude/settings.json.
#[tauri::command]
pub fn mcp_remove_server(id: String) -> Result<(), String> {
    let mut settings = read_settings()?;
    if let Some(servers) = settings.get_mut("mcpServers") {
        if let Some(map) = servers.as_object_mut() {
            map.remove(&id);
        }
    }
    write_settings(&settings)
}

/// Reads an arbitrary file from the filesystem by absolute path.
/// Returns the file contents as a string.
#[tauri::command]
pub fn read_text_file(path: String) -> Result<String, String> {
    let target = std::path::PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("Path is required".to_string());
    }
    if !target.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&target)
        .map_err(|e| format!("Failed to read file: {e}"))
}
