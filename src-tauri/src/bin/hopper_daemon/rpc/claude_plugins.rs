use super::*;
use crate::shared::claude_plugins_core;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginListParams {
    workspace_id: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginActionParams {
    workspace_id: Option<String>,
    action: claude_plugins_core::PluginAction,
    plugin: String,
    scope: claude_plugins_core::PluginScope,
}

pub(super) async fn try_handle(
    state: &DaemonState,
    method: &str,
    params: &Value,
) -> Option<Result<Value, String>> {
    match method {
        "claude_plugins_list" => Some(
            async {
                let params: PluginListParams = serde_json::from_value(params.clone())
                    .map_err(|error| format!("Invalid Claude plugin list parameters: {error}"))?;
                let result = claude_plugins_core::claude_plugins_list_core(
                    &state.workspaces,
                    params.workspace_id,
                )
                .await?;
                serde_json::to_value(result).map_err(|error| error.to_string())
            }
            .await,
        ),
        "claude_plugin_action" => Some(
            async {
                let params: PluginActionParams = serde_json::from_value(params.clone())
                    .map_err(|error| format!("Invalid Claude plugin action parameters: {error}"))?;
                let result = claude_plugins_core::claude_plugin_action_core(
                    &state.workspaces,
                    params.workspace_id,
                    params.action,
                    params.plugin,
                    params.scope,
                )
                .await?;
                serde_json::to_value(result).map_err(|error| error.to_string())
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
    fn validates_rpc_arguments_without_executing_plugin_commands() {
        for params in [
            json!({"plugin":"name@market", "action":"shell", "scope":"user"}),
            json!({"plugin":"name@market", "action":"install", "scope":"managed"}),
            json!({"plugin":"name@market", "action":"install", "scope":"user", "workspaceId":42}),
            json!({"plugin":"name@market", "action":"install", "scope":"user", "args":["--yes"]}),
        ] {
            assert!(serde_json::from_value::<PluginActionParams>(params).is_err());
        }
        assert!(serde_json::from_value::<PluginActionParams>(json!({"plugin":"name@market", "action":"enable", "scope":"local", "workspaceId":"workspace"})).is_ok());
        assert!(serde_json::from_value::<PluginListParams>(json!({})).is_ok());
        assert!(serde_json::from_value::<PluginListParams>(json!({"workspaceId":false})).is_err());
    }
}
