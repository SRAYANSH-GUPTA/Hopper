use super::*;
use crate::shared::provider_setup_core;

pub(super) async fn try_handle(method: &str, params: &Value) -> Option<Result<Value, String>> {
    match method {
        "provider_setup_status" => Some(provider_setup_core::setup_status().await),
        "provider_setup_action" => Some(
            async {
                let provider =
                    serde_json::from_value(params.get("provider").cloned().unwrap_or(Value::Null))
                        .map_err(|_| "Unknown provider".to_string())?;
                let action = parse_string(params, "action")?;
                provider_setup_core::setup_action(provider, &action, true).await
            }
            .await,
        ),
        "provider_setup_save" => Some(
            async {
                let preferences = serde_json::from_value(
                    params.get("preferences").cloned().unwrap_or(Value::Null),
                )
                .map_err(|_| "Invalid setup preferences".to_string())?;
                provider_setup_core::save_preferences(preferences).await
            }
            .await,
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_setup_requests_without_running_commands() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                assert!(try_handle(
                    "provider_setup_action",
                    &json!({"provider": "shell", "action": "install"})
                )
                .await
                .unwrap()
                .is_err());
                assert!(try_handle(
                    "provider_setup_action",
                    &json!({"provider": "claude", "action": "arbitrary"})
                )
                .await
                .unwrap()
                .is_err());
                assert!(
                    try_handle("provider_setup_save", &json!({"preferences": "invalid"}))
                        .await
                        .unwrap()
                        .is_err()
                );
                assert!(try_handle("other_method", &json!({})).await.is_none());
            });
    }
}
