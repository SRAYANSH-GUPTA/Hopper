use serde_json::json;
use tauri::{AppHandle, State};

use crate::remote_backend;
use crate::shared::bridge_core::{
    self, BridgeArtifactContent, BridgeCaptureInput, BridgeImport, BridgeImportSummary,
    BridgeMaterializeResult,
};
use crate::state::AppState;

fn data_dir(state: &AppState) -> Result<&std::path::Path, String> {
    state
        .settings_path
        .parent()
        .ok_or_else(|| "Unable to resolve app data directory.".to_string())
}

#[tauri::command]
pub(crate) async fn bridge_import_file(
    path: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<BridgeImport, String> {
    if remote_backend::is_remote_mode(&state).await {
        let value =
            remote_backend::call_remote(&state, app, "bridge_import_file", json!({ "path": path }))
                .await?;
        return serde_json::from_value(value).map_err(|err| err.to_string());
    }
    bridge_core::bridge_import_file_core(data_dir(&state)?, path).await
}

#[tauri::command]
pub(crate) async fn bridge_import_capture(
    input: BridgeCaptureInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<BridgeImport, String> {
    if remote_backend::is_remote_mode(&state).await {
        let value = remote_backend::call_remote(
            &state,
            app,
            "bridge_import_capture",
            json!({ "input": input }),
        )
        .await?;
        return serde_json::from_value(value).map_err(|err| err.to_string());
    }
    bridge_core::bridge_import_capture_core(data_dir(&state)?, input).await
}

#[tauri::command]
pub(crate) async fn bridge_list_imports(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<BridgeImportSummary>, String> {
    if remote_backend::is_remote_mode(&state).await {
        let value =
            remote_backend::call_remote(&state, app, "bridge_list_imports", json!({})).await?;
        return serde_json::from_value(value).map_err(|err| err.to_string());
    }
    bridge_core::bridge_list_imports_core(data_dir(&state)?).await
}

#[tauri::command]
pub(crate) async fn bridge_get_import(
    import_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<BridgeImport, String> {
    if remote_backend::is_remote_mode(&state).await {
        let value = remote_backend::call_remote(
            &state,
            app,
            "bridge_get_import",
            json!({ "importId": import_id }),
        )
        .await?;
        return serde_json::from_value(value).map_err(|err| err.to_string());
    }
    bridge_core::bridge_get_import_core(data_dir(&state)?, import_id).await
}

#[tauri::command]
pub(crate) async fn bridge_read_artifact(
    import_id: String,
    artifact_path: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<BridgeArtifactContent, String> {
    if remote_backend::is_remote_mode(&state).await {
        let value = remote_backend::call_remote(
            &state,
            app,
            "bridge_read_artifact",
            json!({ "importId": import_id, "artifactPath": artifact_path }),
        )
        .await?;
        return serde_json::from_value(value).map_err(|err| err.to_string());
    }
    bridge_core::bridge_read_artifact_core(data_dir(&state)?, import_id, artifact_path).await
}

#[tauri::command]
pub(crate) async fn bridge_materialize_import(
    import_id: String,
    workspace_id: String,
    subdirectory: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<BridgeMaterializeResult, String> {
    if remote_backend::is_remote_mode(&state).await {
        let value = remote_backend::call_remote(
            &state,
            app,
            "bridge_materialize_import",
            json!({
                "importId": import_id,
                "workspaceId": workspace_id,
                "subdirectory": subdirectory,
            }),
        )
        .await?;
        return serde_json::from_value(value).map_err(|err| err.to_string());
    }
    bridge_core::bridge_materialize_import_core(
        data_dir(&state)?,
        &state.workspaces,
        import_id,
        workspace_id,
        subdirectory,
    )
    .await
}
