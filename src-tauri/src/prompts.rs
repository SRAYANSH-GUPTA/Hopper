use serde_json::{json, Value};
use tauri::{AppHandle, State};

use crate::remote_backend;
use crate::shared::prompts_core::{self, CustomPromptEntry};
use crate::shared::slash_commands_core;
use crate::state::AppState;

/// Skills and custom commands the provider's CLI exposes, for the composer's `/` menu.
#[tauri::command]
pub(crate) async fn slash_commands_list(
    state: State<'_, AppState>,
    app: AppHandle,
    workspace_id: Option<String>,
    provider: String,
) -> Result<Value, String> {
    if remote_backend::is_remote_mode(&*state).await {
        return remote_backend::call_remote(
            &*state,
            app,
            "slash_commands_list",
            json!({ "workspaceId": workspace_id, "provider": provider }),
        )
        .await;
    }
    let commands =
        slash_commands_core::slash_commands_list_core(&state.workspaces, workspace_id, provider)
            .await?;
    serde_json::to_value(commands).map_err(|err| err.to_string())
}

#[tauri::command]
pub(crate) async fn prompts_list(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<Vec<CustomPromptEntry>, String> {
    prompts_core::prompts_list_core(&state.workspaces, &state.settings_path, workspace_id).await
}

#[tauri::command]
pub(crate) async fn prompts_workspace_dir(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<String, String> {
    prompts_core::prompts_workspace_dir_core(&state.workspaces, &state.settings_path, workspace_id)
        .await
}

#[tauri::command]
pub(crate) async fn prompts_global_dir(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<String, String> {
    prompts_core::prompts_global_dir_core(&state.workspaces, workspace_id).await
}

#[tauri::command]
pub(crate) async fn prompts_create(
    state: State<'_, AppState>,
    workspace_id: String,
    scope: String,
    name: String,
    description: Option<String>,
    argument_hint: Option<String>,
    content: String,
) -> Result<CustomPromptEntry, String> {
    prompts_core::prompts_create_core(
        &state.workspaces,
        &state.settings_path,
        workspace_id,
        scope,
        name,
        description,
        argument_hint,
        content,
    )
    .await
}

#[tauri::command]
pub(crate) async fn prompts_update(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
    name: String,
    description: Option<String>,
    argument_hint: Option<String>,
    content: String,
) -> Result<CustomPromptEntry, String> {
    prompts_core::prompts_update_core(
        &state.workspaces,
        &state.settings_path,
        workspace_id,
        path,
        name,
        description,
        argument_hint,
        content,
    )
    .await
}

#[tauri::command]
pub(crate) async fn prompts_delete(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
) -> Result<(), String> {
    prompts_core::prompts_delete_core(&state.workspaces, &state.settings_path, workspace_id, path)
        .await
}

#[tauri::command]
pub(crate) async fn prompts_move(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
    scope: String,
) -> Result<CustomPromptEntry, String> {
    prompts_core::prompts_move_core(
        &state.workspaces,
        &state.settings_path,
        workspace_id,
        path,
        scope,
    )
    .await
}
