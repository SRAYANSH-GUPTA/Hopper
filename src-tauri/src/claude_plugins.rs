use tauri::{AppHandle, State};

use crate::remote_backend;
use crate::shared::claude_plugins_core::{
    self, ClaudePluginsResult, PluginAction, PluginActionResult, PluginScope,
};
use crate::state::AppState;

#[tauri::command]
pub(crate) async fn claude_plugins_list(
    workspace_id: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<ClaudePluginsResult, String> {
    if remote_backend::is_remote_mode(&state).await {
        let result = remote_backend::call_remote(
            &state,
            app,
            "claude_plugins_list",
            serde_json::json!({"workspaceId":workspace_id}),
        )
        .await?;
        return serde_json::from_value(result).map_err(|error| error.to_string());
    }
    claude_plugins_core::claude_plugins_list_core(&state.workspaces, workspace_id).await
}

#[tauri::command]
pub(crate) async fn claude_plugin_action(
    workspace_id: Option<String>,
    action: PluginAction,
    plugin: String,
    scope: PluginScope,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<PluginActionResult, String> {
    if remote_backend::is_remote_mode(&state).await {
        let result = remote_backend::call_remote(&state, app, "claude_plugin_action", serde_json::json!({"workspaceId":workspace_id,"action":action,"plugin":plugin,"scope":scope})).await?;
        return serde_json::from_value(result).map_err(|error| error.to_string());
    }
    claude_plugins_core::claude_plugin_action_core(
        &state.workspaces,
        workspace_id,
        action,
        plugin,
        scope,
    )
    .await
}
