use tauri::{AppHandle, State};

use crate::remote_backend;
use crate::shared::provider_usage_core;
use crate::state::AppState;

#[tauri::command]
pub(crate) async fn provider_usage_output(
    provider: String,
    workspace_path: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<String, String> {
    if remote_backend::is_remote_mode(&*state).await {
        let response = remote_backend::call_remote(
            &*state,
            app,
            "provider_usage_output",
            serde_json::json!({
                "provider": provider,
                "workspacePath": workspace_path,
            }),
        )
        .await?;
        return serde_json::from_value(response).map_err(|err| err.to_string());
    }

    provider_usage_core::provider_usage_output_core(&provider, workspace_path).await
}
