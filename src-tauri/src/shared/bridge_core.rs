use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use tokio::sync::Mutex;
use tokio::task;
use uuid::Uuid;

use crate::types::WorkspaceEntry;

const MAX_MESSAGES: usize = 10_000;
const MAX_CONVERSATION_BYTES: usize = 8 * 1024 * 1024;
const MAX_ARTIFACTS: usize = 200;
const MAX_ARTIFACT_BYTES: usize = 25 * 1024 * 1024;
const MAX_TOTAL_ARTIFACT_BYTES: usize = 100 * 1024 * 1024;
const MAX_IMPORT_FILE_BYTES: u64 = 110 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeMessage {
    pub(crate) role: String,
    pub(crate) content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) created_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeConversation {
    pub(crate) messages: Vec<BridgeMessage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeArtifactInput {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) mime_type: Option<String>,
    pub(crate) content_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeCaptureInput {
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) source_url: Option<String>,
    pub(crate) conversation: BridgeConversation,
    #[serde(default)]
    pub(crate) artifacts: Vec<BridgeArtifactInput>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeArtifactEntry {
    pub(crate) path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mime_type: Option<String>,
    pub(crate) size_bytes: u64,
    #[serde(default)]
    pub(crate) sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeImportSummary {
    pub(crate) id: String,
    pub(crate) source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_url: Option<String>,
    pub(crate) created_at: String,
    pub(crate) message_count: usize,
    pub(crate) artifacts: Vec<BridgeArtifactEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeImport {
    #[serde(flatten)]
    pub(crate) summary: BridgeImportSummary,
    pub(crate) conversation: BridgeConversation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeArtifactContent {
    pub(crate) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mime_type: Option<String>,
    pub(crate) size_bytes: u64,
    pub(crate) content_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeMaterializeResult {
    pub(crate) path: String,
    pub(crate) artifact_count: usize,
}

fn bridge_root(data_dir: &Path) -> PathBuf {
    data_dir.join("bridge").join("imports")
}

fn validate_short_text(value: &str, label: &str, max_bytes: usize) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} is required."));
    }
    if trimmed.len() > max_bytes || trimmed.contains('\0') {
        return Err(format!("{label} is invalid."));
    }
    Ok(trimmed.to_string())
}

fn validate_import_id(import_id: &str) -> Result<String, String> {
    Uuid::parse_str(import_id)
        .map(|value| value.to_string())
        .map_err(|_| "Invalid bridge import id.".to_string())
}

fn safe_relative_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.len() > 1024 || value.contains('\0') {
        return Err("Invalid artifact path.".to_string());
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err("Artifact paths must be relative.".to_string());
    }
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) if part != "." && part != ".." => safe.push(part),
            _ => return Err("Artifact path contains an unsafe component.".to_string()),
        }
    }
    if safe.as_os_str().is_empty() {
        return Err("Artifact path is required.".to_string());
    }
    Ok(safe)
}

fn write_new_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| format!("Unable to create {}: {err}", path.display()))?;
    file.write_all(contents).map_err(|err| err.to_string())
}

fn ensure_no_symlink_components(root: &Path, relative: &Path) -> Result<(), String> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("Materialization path contains a symbolic link.".to_string());
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
            Err(err) => return Err(err.to_string()),
        }
    }
    Ok(())
}

fn infer_mime_type(path: &str) -> Option<String> {
    let extension = Path::new(path).extension()?.to_str()?.to_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "odt" => "application/vnd.oasis.opendocument.text",
        "rtf" => "application/rtf",
        "csv" => "text/csv",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "json" => "application/json",
        "html" => "text/html",
        "css" => "text/css",
        "js" | "jsx" => "text/javascript",
        "ts" | "tsx" => "text/typescript",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        _ => return None,
    };
    Some(mime.to_string())
}

fn standalone_file_capture(
    file_name: String,
    bytes: Vec<u8>,
    conversation_content: Option<String>,
) -> Result<BridgeCaptureInput, String> {
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "Generated file is too large. Bridge accepts individual files up to {} MB.",
            MAX_ARTIFACT_BYTES / (1024 * 1024)
        ));
    }
    let artifact_path = safe_relative_path(&file_name)?
        .to_string_lossy()
        .replace('\\', "/");
    let message = conversation_content.unwrap_or_else(|| {
        format!(
            "Imported generated file `{file_name}` for review. Hopper preserved the original file and did not open or execute it."
        )
    });
    Ok(BridgeCaptureInput {
        source: "file-import".to_string(),
        title: Some(file_name),
        source_url: None,
        conversation: BridgeConversation {
            messages: vec![BridgeMessage {
                role: "user".to_string(),
                content: message,
                created_at: None,
            }],
        },
        artifacts: vec![BridgeArtifactInput {
            mime_type: infer_mime_type(&artifact_path),
            path: artifact_path,
            content_base64: BASE64_STANDARD.encode(bytes),
        }],
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|_| "Bridge import not found.".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "Bridge import is invalid.".to_string())
}

fn import_dir(data_dir: &Path, import_id: &str) -> Result<PathBuf, String> {
    Ok(bridge_root(data_dir).join(validate_import_id(import_id)?))
}

fn normalize_message(value: &serde_json::Value) -> Option<BridgeMessage> {
    let role = value.get("role")?.as_str()?.trim().to_lowercase();
    if !matches!(role.as_str(), "user" | "assistant" | "system" | "tool") {
        return None;
    }
    let content = match value.get("content")? {
        serde_json::Value::String(content) => content.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.as_str()
                    .map(str::to_string)
                    .or_else(|| part.get("text")?.as_str().map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    Some(BridgeMessage {
        role,
        content,
        created_at: value
            .get("createdAt")
            .or_else(|| value.get("created_at"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
    })
}

fn parse_conversation(value: &serde_json::Value) -> Result<BridgeConversation, String> {
    let messages = value
        .get("messages")
        .and_then(|value| value.as_array())
        .or_else(|| value.as_array())
        .ok_or_else(|| "JSON import must contain a messages array.".to_string())?
        .iter()
        .filter_map(normalize_message)
        .collect::<Vec<_>>();
    if messages.is_empty() {
        return Err("JSON import contains no supported messages.".to_string());
    }
    Ok(BridgeConversation { messages })
}

fn capture_from_json(bytes: &[u8], fallback_title: &str) -> Result<BridgeCaptureInput, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Selected JSON file is invalid.".to_string())?;
    if let Ok(mut input) = serde_json::from_value::<BridgeCaptureInput>(value.clone()) {
        if input.title.is_none() {
            input.title = Some(fallback_title.to_string());
        }
        return Ok(input);
    }
    let manifest = value.get("manifest").unwrap_or(&value);
    let conversation_value = value.get("conversation").unwrap_or(&value);
    Ok(BridgeCaptureInput {
        source: manifest
            .get("source")
            .and_then(|value| value.as_str())
            .unwrap_or("file-import")
            .to_string(),
        title: manifest
            .get("title")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| Some(fallback_title.to_string())),
        source_url: manifest
            .get("sourceUrl")
            .or_else(|| manifest.get("source_url"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        conversation: parse_conversation(conversation_value)?,
        artifacts: Vec::new(),
    })
}

fn capture_from_zip(bytes: Vec<u8>, fallback_title: &str) -> Result<BridgeCaptureInput, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "Selected ZIP is invalid.".to_string())?;
    if archive.len() > MAX_ARTIFACTS + 2 {
        return Err("Bundle contains too many files.".to_string());
    }
    let mut files = HashMap::<String, Vec<u8>>::new();
    let mut total_size = 0usize;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|err| err.to_string())?;
        if entry.is_dir() {
            continue;
        }
        if entry.size() as usize > MAX_ARTIFACT_BYTES {
            return Err(format!("Bundle entry {} is too large.", entry.name()));
        }
        let raw_name = entry.name().replace('\\', "/");
        let trimmed_name = raw_name
            .strip_prefix(".hopper-bundle/")
            .unwrap_or(&raw_name);
        let path = safe_relative_path(trimmed_name)?;
        let normalized = path.to_string_lossy().replace('\\', "/");
        let mut contents = Vec::with_capacity(entry.size() as usize);
        entry
            .take((MAX_ARTIFACT_BYTES + 1) as u64)
            .read_to_end(&mut contents)
            .map_err(|err| err.to_string())?;
        total_size = total_size
            .checked_add(contents.len())
            .ok_or_else(|| "Bundle is too large.".to_string())?;
        if total_size > MAX_TOTAL_ARTIFACT_BYTES + MAX_CONVERSATION_BYTES {
            return Err("Bundle is too large.".to_string());
        }
        if files.insert(normalized.clone(), contents).is_some() {
            return Err(format!("Bundle contains duplicate path {normalized}."));
        }
    }

    let conversation_bytes = files
        .remove("conversation.json")
        .ok_or_else(|| "Bundle is missing conversation.json.".to_string())?;
    let conversation_value: serde_json::Value = serde_json::from_slice(&conversation_bytes)
        .map_err(|_| "Bundle conversation.json is invalid.".to_string())?;
    let manifest_value = files
        .remove("manifest.json")
        .map(|contents| serde_json::from_slice::<serde_json::Value>(&contents))
        .transpose()
        .map_err(|_| "Bundle manifest.json is invalid.".to_string())?
        .unwrap_or_else(|| serde_json::json!({}));
    let artifacts = files
        .into_iter()
        .filter_map(|(path, contents)| {
            let path = path.strip_prefix("artifacts/")?.to_string();
            Some(BridgeArtifactInput {
                mime_type: infer_mime_type(&path),
                path,
                content_base64: BASE64_STANDARD.encode(contents),
            })
        })
        .collect();
    Ok(BridgeCaptureInput {
        source: manifest_value
            .get("source")
            .and_then(|value| value.as_str())
            .unwrap_or("bundle-import")
            .to_string(),
        title: manifest_value
            .get("title")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| Some(fallback_title.to_string())),
        source_url: manifest_value
            .get("sourceUrl")
            .or_else(|| manifest_value.get("source_url"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        conversation: parse_conversation(&conversation_value)?,
        artifacts,
    })
}

pub(crate) async fn bridge_import_file_core(
    data_dir: &Path,
    path: String,
) -> Result<BridgeImport, String> {
    let file_path = PathBuf::from(path);
    let metadata =
        fs::metadata(&file_path).map_err(|_| "Selected import file was not found.".to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_IMPORT_FILE_BYTES {
        return Err("Selected import file is too large or is not a file.".to_string());
    }
    let title = file_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Imported file")
        .to_string();
    let extension = file_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !matches!(extension.as_str(), "json" | "zip") && metadata.len() > MAX_ARTIFACT_BYTES as u64 {
        return Err(format!(
            "Generated file is too large. Bridge accepts individual files up to {} MB.",
            MAX_ARTIFACT_BYTES / (1024 * 1024)
        ));
    }
    let bytes = task::spawn_blocking(move || fs::read(file_path))
        .await
        .map_err(|_| "Bridge file read task failed.".to_string())?
        .map_err(|err| err.to_string())?;
    let input = match extension.as_str() {
        "json" => capture_from_json(&bytes, &title)?,
        "zip" => capture_from_zip(bytes, &title)?,
        "md" | "markdown" | "txt" => {
            let content = String::from_utf8(bytes.clone())
                .map_err(|_| "Selected text file is not valid UTF-8.".to_string())?;
            standalone_file_capture(title, bytes, Some(content))?
        }
        _ => standalone_file_capture(title, bytes, None)?,
    };
    bridge_import_capture_core(data_dir, input).await
}

pub(crate) async fn bridge_import_capture_core(
    data_dir: &Path,
    input: BridgeCaptureInput,
) -> Result<BridgeImport, String> {
    let data_dir = data_dir.to_path_buf();
    task::spawn_blocking(move || {
        let source = validate_short_text(&input.source, "Capture source", 64)?;
        let title = input
            .title
            .map(|value| validate_short_text(&value, "Capture title", 512))
            .transpose()?;
        let source_url = input
            .source_url
            .map(|value| validate_short_text(&value, "Source URL", 4096))
            .transpose()?;
        if input.conversation.messages.len() > MAX_MESSAGES {
            return Err("Capture contains too many messages.".to_string());
        }
        for message in &input.conversation.messages {
            if !matches!(
                message.role.as_str(),
                "user" | "assistant" | "system" | "tool"
            ) {
                return Err(format!("Unsupported conversation role: {}.", message.role));
            }
            if message.content.contains('\0') {
                return Err("Conversation contains invalid text.".to_string());
            }
        }
        let conversation_bytes =
            serde_json::to_vec_pretty(&input.conversation).map_err(|err| err.to_string())?;
        if conversation_bytes.len() > MAX_CONVERSATION_BYTES {
            return Err("Capture conversation is too large.".to_string());
        }
        if input.artifacts.len() > MAX_ARTIFACTS {
            return Err("Capture contains too many artifacts.".to_string());
        }

        let mut decoded_artifacts = Vec::with_capacity(input.artifacts.len());
        let mut artifact_entries = Vec::with_capacity(input.artifacts.len());
        let mut total_size = 0usize;
        for artifact in input.artifacts {
            let relative_path = safe_relative_path(&artifact.path)?;
            if decoded_artifacts
                .iter()
                .any(|(existing, _): &(PathBuf, Vec<u8>)| existing == &relative_path)
            {
                return Err("Capture contains duplicate artifact paths.".to_string());
            }
            let bytes = BASE64_STANDARD
                .decode(artifact.content_base64.as_bytes())
                .map_err(|_| format!("Artifact {} is not valid base64.", artifact.path))?;
            if bytes.len() > MAX_ARTIFACT_BYTES {
                return Err(format!("Artifact {} is too large.", artifact.path));
            }
            total_size = total_size
                .checked_add(bytes.len())
                .ok_or_else(|| "Capture artifacts are too large.".to_string())?;
            if total_size > MAX_TOTAL_ARTIFACT_BYTES {
                return Err("Capture artifacts are too large.".to_string());
            }
            let normalized = relative_path.to_string_lossy().replace('\\', "/");
            artifact_entries.push(BridgeArtifactEntry {
                path: normalized,
                mime_type: artifact.mime_type,
                size_bytes: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
            });
            decoded_artifacts.push((relative_path, bytes));
        }

        let id = Uuid::new_v4().to_string();
        let summary = BridgeImportSummary {
            id: id.clone(),
            source,
            title,
            source_url,
            created_at: Utc::now().to_rfc3339(),
            message_count: input.conversation.messages.len(),
            artifacts: artifact_entries,
        };
        let root = bridge_root(&data_dir);
        fs::create_dir_all(&root).map_err(|err| err.to_string())?;
        let temporary_dir = root.join(format!(".{id}.tmp"));
        let final_dir = root.join(&id);
        fs::create_dir(&temporary_dir).map_err(|err| err.to_string())?;
        let write_result = (|| {
            write_new_file(
                &temporary_dir.join("manifest.json"),
                &serde_json::to_vec_pretty(&summary).map_err(|err| err.to_string())?,
            )?;
            write_new_file(
                &temporary_dir.join("conversation.json"),
                &conversation_bytes,
            )?;
            for (path, bytes) in decoded_artifacts {
                write_new_file(&temporary_dir.join("artifacts").join(path), &bytes)?;
            }
            fs::rename(&temporary_dir, &final_dir).map_err(|err| err.to_string())
        })();
        if write_result.is_err() {
            let _ = fs::remove_dir_all(&temporary_dir);
        }
        write_result?;
        Ok(BridgeImport {
            summary,
            conversation: input.conversation,
        })
    })
    .await
    .map_err(|_| "Bridge import task failed.".to_string())?
}

pub(crate) async fn bridge_list_imports_core(
    data_dir: &Path,
) -> Result<Vec<BridgeImportSummary>, String> {
    let data_dir = data_dir.to_path_buf();
    task::spawn_blocking(move || {
        let root = bridge_root(&data_dir);
        let Ok(entries) = fs::read_dir(root) else {
            return Ok(Vec::new());
        };
        let mut imports = entries
            .flatten()
            .filter_map(|entry| {
                read_json::<BridgeImportSummary>(&entry.path().join("manifest.json")).ok()
            })
            .collect::<Vec<_>>();
        imports.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(imports)
    })
    .await
    .map_err(|_| "Bridge list task failed.".to_string())?
}

pub(crate) async fn bridge_get_import_core(
    data_dir: &Path,
    import_id: String,
) -> Result<BridgeImport, String> {
    let data_dir = data_dir.to_path_buf();
    task::spawn_blocking(move || {
        let dir = import_dir(&data_dir, &import_id)?;
        Ok(BridgeImport {
            summary: read_json(&dir.join("manifest.json"))?,
            conversation: read_json(&dir.join("conversation.json"))?,
        })
    })
    .await
    .map_err(|_| "Bridge read task failed.".to_string())?
}

pub(crate) async fn bridge_read_artifact_core(
    data_dir: &Path,
    import_id: String,
    artifact_path: String,
) -> Result<BridgeArtifactContent, String> {
    let data_dir = data_dir.to_path_buf();
    task::spawn_blocking(move || {
        let dir = import_dir(&data_dir, &import_id)?;
        let manifest: BridgeImportSummary = read_json(&dir.join("manifest.json"))?;
        let relative_path = safe_relative_path(&artifact_path)?;
        let normalized = relative_path.to_string_lossy().replace('\\', "/");
        let entry = manifest
            .artifacts
            .iter()
            .find(|entry| entry.path == normalized)
            .ok_or_else(|| "Bridge artifact not found.".to_string())?;
        let bytes = fs::read(dir.join("artifacts").join(relative_path))
            .map_err(|_| "Bridge artifact not found.".to_string())?;
        Ok(BridgeArtifactContent {
            path: normalized,
            mime_type: entry.mime_type.clone(),
            size_bytes: bytes.len() as u64,
            content_base64: BASE64_STANDARD.encode(bytes),
        })
    })
    .await
    .map_err(|_| "Bridge artifact read task failed.".to_string())?
}

fn materialize_dir_name(summary: &BridgeImportSummary) -> String {
    let base = summary.title.as_deref().unwrap_or("ai-web-import");
    let mut sanitized = base
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    sanitized = sanitized.trim_matches(['-', '.']).to_string();
    if sanitized.is_empty() {
        sanitized = "ai-web-import".to_string();
    }
    sanitized.truncate(64);
    format!("{sanitized}-{}", &summary.id[..8])
}

pub(crate) async fn bridge_materialize_import_core(
    data_dir: &Path,
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    import_id: String,
    workspace_id: String,
    subdirectory: Option<String>,
) -> Result<BridgeMaterializeResult, String> {
    let workspace_path = {
        let workspaces = workspaces.lock().await;
        workspaces
            .get(&workspace_id)
            .map(|entry| PathBuf::from(&entry.path))
            .ok_or_else(|| "Workspace not found.".to_string())?
    };
    let data_dir = data_dir.to_path_buf();
    task::spawn_blocking(move || {
        if !workspace_path.is_dir() {
            return Err("Workspace path is not a directory.".to_string());
        }
        let source_dir = import_dir(&data_dir, &import_id)?;
        let summary: BridgeImportSummary = read_json(&source_dir.join("manifest.json"))?;
        let relative_destination = match subdirectory {
            Some(value) => safe_relative_path(&value)?,
            None => PathBuf::from(".hopper")
                .join("imports")
                .join(materialize_dir_name(&summary)),
        };
        let destination = workspace_path.join(&relative_destination);
        ensure_no_symlink_components(&workspace_path, &relative_destination)?;
        fs::create_dir(&destination).map_err(|err| {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                "Materialization destination already exists; no files were overwritten.".to_string()
            } else {
                format!("Unable to create materialization destination: {err}")
            }
        })?;
        let result: Result<(), String> = (|| {
            write_new_file(
                &destination.join("manifest.json"),
                &serde_json::to_vec_pretty(&summary).map_err(|err| err.to_string())?,
            )?;
            let conversation =
                fs::read(source_dir.join("conversation.json")).map_err(|err| err.to_string())?;
            write_new_file(&destination.join("conversation.json"), &conversation)?;
            for artifact in &summary.artifacts {
                let path = safe_relative_path(&artifact.path)?;
                let bytes = fs::read(source_dir.join("artifacts").join(&path))
                    .map_err(|err| err.to_string())?;
                write_new_file(&destination.join("artifacts").join(path), &bytes)?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&destination);
        }
        result?;
        Ok(BridgeMaterializeResult {
            path: destination.to_string_lossy().to_string(),
            artifact_count: summary.artifacts.len(),
        })
    })
    .await
    .map_err(|_| "Bridge materialization task failed.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("hopper-bridge-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create test data directory");
        path
    }

    #[test]
    fn rejects_path_traversal_and_absolute_paths() {
        assert!(safe_relative_path("../secret").is_err());
        assert!(safe_relative_path("nested/../../secret").is_err());
        assert!(safe_relative_path("/tmp/secret").is_err());
        assert_eq!(
            safe_relative_path("src/App.tsx").unwrap(),
            PathBuf::from("src/App.tsx")
        );
    }

    #[test]
    fn parses_hopper_bundle_zip() {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file(".hopper-bundle/manifest.json", options)
                .expect("start manifest");
            writer
                .write_all(br#"{"source":"claude-web","title":"Design"}"#)
                .expect("write manifest");
            writer
                .start_file(".hopper-bundle/conversation.json", options)
                .expect("start conversation");
            writer
                .write_all(br#"{"messages":[{"role":"user","content":"Build it"}]}"#)
                .expect("write conversation");
            writer
                .start_file(".hopper-bundle/artifacts/index.html", options)
                .expect("start artifact");
            writer
                .write_all(b"<main>Hello</main>")
                .expect("write artifact");
            writer.finish().expect("finish archive");
        }

        let capture = capture_from_zip(cursor.into_inner(), "fallback").expect("parse bundle");
        assert_eq!(capture.source, "claude-web");
        assert_eq!(capture.title.as_deref(), Some("Design"));
        assert_eq!(capture.conversation.messages[0].content, "Build it");
        assert_eq!(capture.artifacts[0].path, "index.html");
        assert_eq!(capture.artifacts[0].mime_type.as_deref(), Some("text/html"));
    }

    #[test]
    fn imports_capture_with_artifact_checksum() {
        let data_dir = test_data_dir();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let imported = runtime
            .block_on(bridge_import_capture_core(
                &data_dir,
                BridgeCaptureInput {
                    source: "chatgpt".to_string(),
                    title: Some("Landing page".to_string()),
                    source_url: None,
                    conversation: BridgeConversation {
                        messages: vec![BridgeMessage {
                            role: "user".to_string(),
                            content: "Build this".to_string(),
                            created_at: None,
                        }],
                    },
                    artifacts: vec![BridgeArtifactInput {
                        path: "src/App.tsx".to_string(),
                        mime_type: Some("text/typescript".to_string()),
                        content_base64: BASE64_STANDARD.encode("export default null;"),
                    }],
                },
            ))
            .expect("import capture");

        assert_eq!(imported.summary.message_count, 1);
        assert_eq!(imported.summary.artifacts.len(), 1);
        assert_eq!(imported.summary.artifacts[0].sha256.len(), 64);
        assert!(data_dir
            .join("bridge/imports")
            .join(imported.summary.id)
            .join("artifacts/src/App.tsx")
            .is_file());
        fs::remove_dir_all(data_dir).expect("remove test data directory");
    }

    #[test]
    fn imports_markdown_without_touching_a_workspace() {
        let data_dir = test_data_dir();
        let markdown_path = data_dir.join("conversation.md");
        fs::write(&markdown_path, "# Build a dashboard").expect("write markdown fixture");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let imported = runtime
            .block_on(bridge_import_file_core(
                &data_dir,
                markdown_path.to_string_lossy().to_string(),
            ))
            .expect("import markdown");

        assert_eq!(imported.summary.source, "file-import");
        assert_eq!(
            imported.conversation.messages[0].content,
            "# Build a dashboard"
        );
        assert_eq!(imported.summary.artifacts.len(), 1);
        assert_eq!(imported.summary.artifacts[0].path, "conversation.md");
        assert!(!data_dir.join(".hopper").exists());
        fs::remove_dir_all(data_dir).expect("remove test data directory");
    }

    #[test]
    fn imports_generated_document_as_unchanged_artifact() {
        let data_dir = test_data_dir();
        let document_path = data_dir.join("ChatGPT project brief.docx");
        let document_bytes = b"PK\x03\x04generated-document";
        fs::write(&document_path, document_bytes).expect("write document fixture");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let imported = runtime
            .block_on(bridge_import_file_core(
                &data_dir,
                document_path.to_string_lossy().to_string(),
            ))
            .expect("import generated document");

        assert_eq!(
            imported.summary.title.as_deref(),
            Some("ChatGPT project brief.docx")
        );
        assert_eq!(imported.summary.artifacts.len(), 1);
        let artifact = &imported.summary.artifacts[0];
        assert_eq!(artifact.path, "ChatGPT project brief.docx");
        assert_eq!(
            artifact.mime_type.as_deref(),
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        );
        assert_eq!(
            artifact.sha256,
            format!("{:x}", Sha256::digest(document_bytes))
        );
        let stored = fs::read(
            data_dir
                .join("bridge/imports")
                .join(imported.summary.id)
                .join("artifacts/ChatGPT project brief.docx"),
        )
        .expect("read stored document");
        assert_eq!(stored, document_bytes);
        assert!(imported.conversation.messages[0]
            .content
            .contains("did not open or execute it"));

        fs::remove_dir_all(data_dir).expect("remove test data directory");
    }
}
