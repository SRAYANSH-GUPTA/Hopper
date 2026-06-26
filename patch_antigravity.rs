use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;
use serde_json::Value;

pub async fn tail_transcript<E: crate::backend::events::EventSink + 'static>(
    workspace_id: String,
    thread_id: String,
    turn_id: String,
    session_id: String,
    event_sink: E,
) {
    let path = dirs::home_dir()
        .unwrap()
        .join(format!(".gemini/antigravity-cli/brain/{}/.system_generated/logs/transcript.jsonl", session_id));
    
    let mut pos = 0;
    loop {
        if let Ok(mut file) = File::open(&path) {
            let _ = file.seek(SeekFrom::Start(pos));
            let mut buf = String::new();
            if let Ok(n) = file.read_to_string(&mut buf) {
                if n > 0 {
                    pos += n as u64;
                    for line in buf.lines() {
                        if let Ok(val) = serde_json::from_str::<Value>(line) {
                            if let Some(t) = val.get("type").and_then(|v| v.as_str()) {
                                if t == "PLANNER_RESPONSE" {
                                    if let Some(thinking) = val.get("thinking").and_then(|v| v.as_str()) {
                                        let item_id = format!("reasoning-{}", val.get("step_index").unwrap_or(&serde_json::json!(0)));
                                        event_sink.emit_app_server_event(crate::backend::events::AppServerEvent {
                                            workspace_id: workspace_id.clone(),
                                            message: serde_json::json!({
                                                "method": "item/started",
                                                "params": {
                                                    "threadId": thread_id,
                                                    "item": {
                                                        "type": "reasoning",
                                                        "id": item_id,
                                                        "turnId": turn_id,
                                                        "summary": "Thinking...",
                                                        "content": ""
                                                    }
                                                }
                                            }),
                                        });
                                        event_sink.emit_app_server_event(crate::backend::events::AppServerEvent {
                                            workspace_id: workspace_id.clone(),
                                            message: serde_json::json!({
                                                "method": "item/reasoning/textDelta",
                                                "params": {
                                                    "threadId": thread_id,
                                                    "itemId": item_id,
                                                    "delta": thinking
                                                }
                                            }),
                                        });
                                        event_sink.emit_app_server_event(crate::backend::events::AppServerEvent {
                                            workspace_id: workspace_id.clone(),
                                            message: serde_json::json!({
                                                "method": "item/completed",
                                                "params": {
                                                    "threadId": thread_id,
                                                    "item": {
                                                        "type": "reasoning",
                                                        "id": item_id,
                                                        "turnId": turn_id,
                                                        "summary": "Thinking...",
                                                        "content": thinking
                                                    }
                                                }
                                            }),
                                        });
                                    }
                                } else if t != "USER_INPUT" && t != "CONVERSATION_HISTORY" && t != "EPHEMERAL_MESSAGE" && t != "CHECKPOINT" {
                                    // Tool execution
                                    let item_id = format!("tool-{}", val.get("step_index").unwrap_or(&serde_json::json!(0)));
                                    let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                                    event_sink.emit_app_server_event(crate::backend::events::AppServerEvent {
                                        workspace_id: workspace_id.clone(),
                                        message: serde_json::json!({
                                            "method": "item/started",
                                            "params": {
                                                "threadId": thread_id,
                                                "item": {
                                                    "type": "commandExecution",
                                                    "id": item_id,
                                                    "turnId": turn_id,
                                                    "command": t,
                                                    "status": "in_progress"
                                                }
                                            }
                                        }),
                                    });
                                    event_sink.emit_app_server_event(crate::backend::events::AppServerEvent {
                                        workspace_id: workspace_id.clone(),
                                        message: serde_json::json!({
                                            "method": "item/commandExecution/outputDelta",
                                            "params": {
                                                "threadId": thread_id,
                                                "itemId": item_id,
                                                "delta": content
                                            }
                                        }),
                                    });
                                    event_sink.emit_app_server_event(crate::backend::events::AppServerEvent {
                                        workspace_id: workspace_id.clone(),
                                        message: serde_json::json!({
                                            "method": "item/completed",
                                            "params": {
                                                "threadId": thread_id,
                                                "item": {
                                                    "type": "commandExecution",
                                                    "id": item_id,
                                                    "turnId": turn_id,
                                                    "command": t,
                                                    "status": "completed",
                                                    "output": content
                                                }
                                            }
                                        }),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
