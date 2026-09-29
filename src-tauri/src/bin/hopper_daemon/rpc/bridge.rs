use super::*;
use crate::shared::bridge_core;

pub(super) async fn try_handle(
    state: &DaemonState,
    method: &str,
    params: &Value,
) -> Option<Result<Value, String>> {
    let result = match method {
        "bridge_import_file" => {
            let path = match parse_string(params, "path") {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            bridge_core::bridge_import_file_core(&state.data_dir, path)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|err| err.to_string()))
        }
        "bridge_import_capture" => {
            let input = match params.get("input").cloned() {
                Some(value) => match serde_json::from_value(value) {
                    Ok(value) => value,
                    Err(err) => return Some(Err(format!("invalid bridge capture: {err}"))),
                },
                None => return Some(Err("missing `input`".to_string())),
            };
            bridge_core::bridge_import_capture_core(&state.data_dir, input)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|err| err.to_string()))
        }
        "bridge_list_imports" => bridge_core::bridge_list_imports_core(&state.data_dir)
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|err| err.to_string())),
        "bridge_get_import" => {
            let import_id = match parse_string(params, "importId") {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            bridge_core::bridge_get_import_core(&state.data_dir, import_id)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|err| err.to_string()))
        }
        "bridge_read_artifact" => {
            let import_id = match parse_string(params, "importId") {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let artifact_path = match parse_string(params, "artifactPath") {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            bridge_core::bridge_read_artifact_core(&state.data_dir, import_id, artifact_path)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|err| err.to_string()))
        }
        "bridge_materialize_import" => {
            let import_id = match parse_string(params, "importId") {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let workspace_id = match parse_string(params, "workspaceId") {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            bridge_core::bridge_materialize_import_core(
                &state.data_dir,
                &state.workspaces,
                import_id,
                workspace_id,
                parse_optional_string(params, "subdirectory"),
            )
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|err| err.to_string()))
        }
        _ => return None,
    };
    Some(result)
}
