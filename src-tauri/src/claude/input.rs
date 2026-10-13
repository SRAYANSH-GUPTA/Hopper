//! Claude CLI stream-json input for native image attachments.
use base64::Engine;
use serde_json::{json, Value};
use std::io::Read;
use tokio::io::AsyncWriteExt;

const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const MAX_TOTAL_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_IMAGES: usize = 8;

fn image_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

fn read_image(image: &str) -> Result<(Vec<u8>, &'static str), String> {
    let (bytes, declared) = if let Some(data) = image.strip_prefix("data:") {
        let (header, encoded) = data.split_once(',').ok_or("Invalid image data URL")?;
        let mime = header
            .strip_suffix(";base64")
            .ok_or("Image attachments must use base64 encoding")?;
        if encoded.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
            return Err("Each image attachment must be 5 MiB or smaller".into());
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "Invalid image base64 data")?;
        (decoded, Some(mime))
    } else {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // A named pipe or device must not stall the app while attaching.
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = options
            .open(image)
            .map_err(|error| format!("Cannot read image attachment {image}: {error}"))?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err("Image attachment must be a regular file".into());
        }
        if metadata.len() > MAX_IMAGE_BYTES as u64 {
            return Err("Each image attachment must be 5 MiB or smaller".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_IMAGE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        (bytes, None)
    };
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("Each image attachment must be 5 MiB or smaller".into());
    }
    let mime = image_media_type(&bytes)
        .ok_or("Unsupported image attachment. Use PNG, JPEG, GIF, or WebP.")?;
    if declared.is_some_and(|declared| {
        declared != mime && !(declared == "image/jpg" && mime == "image/jpeg")
    }) {
        return Err("Image attachment content does not match its declared media type".into());
    }
    Ok((bytes, mime))
}

/// The CLI accepts SDKUserMessage content blocks via --input-format stream-json.
/// Schema: https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode
pub(super) fn image_prompt(text: &str, images: &[String]) -> Result<Vec<u8>, String> {
    if images.len() > MAX_IMAGES {
        return Err("Attach no more than 8 images per message".into());
    }
    let mut content: Vec<Value> = vec![json!({"type":"text", "text":text})];
    let mut total = 0;
    for image in images {
        let (bytes, mime) = read_image(image)?;
        total += bytes.len();
        if total > MAX_TOTAL_IMAGE_BYTES {
            return Err("Image attachments must total 20 MiB or less per message".into());
        }
        content.push(json!({"type":"image", "source": {"type":"base64", "media_type":mime, "data":base64::engine::general_purpose::STANDARD.encode(bytes)}}));
    }
    let mut input = serde_json::to_vec(&json!({"type":"user", "message":{"role":"user", "content":content}, "parent_tool_use_id":null})).map_err(|error| error.to_string())?;
    input.push(b'\n');
    Ok(input)
}

pub(super) async fn write_prompt(
    child: &mut tokio::process::Child,
    bytes: &[u8],
    timeout: std::time::Duration,
) -> Result<(), String> {
    let written = tokio::time::timeout(timeout, async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("Claude stdin is unavailable"))?;
        stdin.write_all(bytes).await?;
        stdin.shutdown().await
    })
    .await;
    match written {
        Ok(Ok(())) => Ok(()),
        result => {
            crate::shared::process_core::kill_child_process_tree(child).await;
            let detail = match result {
                Ok(Err(error)) => error.to_string(),
                _ => "timed out waiting for the CLI to read its input".into(),
            };
            Err(format!("Cannot send image attachments to Claude: {detail}. The process was stopped; try again."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_one_sdk_user_message_with_inline_images_and_preserved_text() {
        let png = b"\x89PNG\r\n\x1a\nfixture";
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let wire =
            image_prompt("Explain\n😀", &[format!("data:image/png;base64,{encoded}")]).unwrap();
        assert_eq!(wire.last(), Some(&b'\n'));
        assert_eq!(wire.iter().filter(|byte| **byte == b'\n').count(), 1);
        let value: Value = serde_json::from_slice(&wire).unwrap();
        assert_eq!(value["type"], "user");
        assert_eq!(value["message"]["role"], "user");
        assert_eq!(value["message"]["content"][0]["text"], "Explain\n😀");
        assert_eq!(
            value["message"]["content"][1]["source"],
            json!({"type":"base64","media_type":"image/png","data":encoded})
        );
        assert!(value["parent_tool_use_id"].is_null());
    }

    #[test]
    fn local_images_are_loaded_by_content_and_invalid_attachments_are_rejected() {
        let path = std::env::temp_dir().join(format!("hopper-input-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"GIF89afixture").unwrap();
        let wire = image_prompt("look", &[path.to_string_lossy().into_owned()]).unwrap();
        let value: Value = serde_json::from_slice(&wire).unwrap();
        assert_eq!(
            value["message"]["content"][1]["source"]["media_type"],
            "image/gif"
        );
        assert!(image_prompt("", &["data:image/svg+xml;base64,PHN2Zz4=".into()]).is_err());
        assert!(image_prompt("", &["data:image/png;base64,%%%".into()]).is_err());
        assert!(image_prompt(
            "",
            &[format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(b"GIF89afixture")
            )]
        )
        .is_err());
        assert!(image_prompt("", &vec!["unused".into(); MAX_IMAGES + 1]).is_err());
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(MAX_IMAGE_BYTES as u64 + 1).unwrap();
        assert!(image_prompt("", &[path.to_string_lossy().into_owned()])
            .unwrap_err()
            .contains("5 MiB"));
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn writes_a_complete_message_and_closes_stdin() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let wire = image_prompt("hello", &[]).unwrap();
                let mut command = crate::shared::process_core::tokio_command("cat");
                command
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped());
                crate::shared::process_core::configure_owned_process(&mut command);
                let mut child = command.spawn().unwrap();
                write_prompt(&mut child, &wire, std::time::Duration::from_secs(2))
                    .await
                    .unwrap();
                let output = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    child.wait_with_output(),
                )
                .await
                .unwrap()
                .unwrap();
                assert_eq!(output.stdout, wire);
                assert!(output.status.success());
            });
    }

    #[cfg(unix)]
    #[test]
    fn stalled_stdin_stops_the_owned_process_and_descendants() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut command = crate::shared::process_core::tokio_command("sh");
                command
                    .args(["-c", "sleep 60 & echo ready; wait"])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped());
                crate::shared::process_core::configure_owned_process(&mut command);
                let mut child = command.spawn().unwrap();
                let pid = child.id().unwrap();
                let mut stdout = BufReader::new(child.stdout.take().unwrap());
                stdout.read_line(&mut String::new()).await.unwrap();
                assert!(write_prompt(
                    &mut child,
                    &vec![b'x'; 1024 * 1024],
                    std::time::Duration::from_millis(30)
                )
                .await
                .unwrap_err()
                .contains("process was stopped"));
                let closed = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    stdout.read_to_end(&mut Vec::new()),
                )
                .await;
                if closed.is_err() {
                    crate::shared::process_core::kill_registered_process_tree(pid).await;
                }
                closed.unwrap().unwrap();
                assert!(child.try_wait().unwrap().is_some());
            });
    }
}
