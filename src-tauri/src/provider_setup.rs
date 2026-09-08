use crate::shared::provider_setup_core::{self, SetupPreferences, SetupProvider};
use crate::{remote_backend, state::AppState};
use serde_json::{json, Value};
use tauri::{AppHandle, State};

#[tauri::command]
pub(crate) async fn provider_setup_status(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    if remote_backend::is_remote_mode(&state).await {
        return remote_backend::call_remote(&state, app, "provider_setup_status", json!({})).await;
    }
    provider_setup_core::setup_status().await
}
#[tauri::command]
pub(crate) async fn provider_setup_action(
    provider: SetupProvider,
    action: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    if remote_backend::is_remote_mode(&state).await {
        return remote_backend::call_remote(
            &state,
            app,
            "provider_setup_action",
            json!({"provider": provider, "action": action}),
        )
        .await;
    }
    provider_setup_core::setup_action(provider, &action, false).await
}
#[tauri::command]
pub(crate) async fn provider_setup_save(
    preferences: SetupPreferences,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    if remote_backend::is_remote_mode(&state).await {
        return remote_backend::call_remote(
            &state,
            app,
            "provider_setup_save",
            json!({"preferences": preferences}),
        )
        .await;
    }
    provider_setup_core::save_preferences(preferences).await
}
