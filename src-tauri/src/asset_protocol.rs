//! Local image previews are available only to Hopper's own webviews.
//!
//! Tauri's default asset handler is installed in external webviews too, and its
//! CORS origin is that webview's initial URL. A filesystem scope alone therefore
//! cannot separate a browser tab from the application's local image previews.
use std::path::Path;

use tauri::http::{Method, Request, Response, StatusCode};
use tauri::utils::mime_type::MimeType;
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder, Url};
use tokio::io::AsyncReadExt;

use crate::web_chat::{is_app_url, is_app_webview};

const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;

fn asset_origin(label: &str, url: &Url, dev_url: Option<&Url>) -> Option<String> {
    if !is_app_webview(label) || !is_app_url(url, dev_url) {
        return None;
    }
    Some(format!(
        "{}://{}{}",
        url.scheme(),
        url.host_str()?,
        url.port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default()
    ))
}

fn error_response(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(status).body(Vec::new()).unwrap()
}

async fn read_image(path: &Path) -> Result<(Vec<u8>, String), StatusCode> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !metadata.is_file() {
        return Err(StatusCode::FORBIDDEN);
    }
    if metadata.len() > MAX_ASSET_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let mut bytes = Vec::new();
    file.take(MAX_ASSET_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if bytes.len() as u64 > MAX_ASSET_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let mime =
        MimeType::parse_with_fallback(&bytes, &path.to_string_lossy(), MimeType::OctetStream);
    if !mime.starts_with("image/") {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    Ok((bytes, mime))
}

pub(crate) fn handle<R: Runtime>(
    context: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = context.app_handle();
    let dev_url = if cfg!(dev) {
        app.config().build.dev_url.as_ref()
    } else {
        None
    };
    let origin = app
        .get_webview(context.webview_label())
        .and_then(|view| view.url().ok())
        .and_then(|url| asset_origin(context.webview_label(), &url, dev_url));
    let Some(origin) = origin else {
        responder.respond(error_response(StatusCode::FORBIDDEN));
        return;
    };
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        responder.respond(error_response(StatusCode::METHOD_NOT_ALLOWED));
        return;
    }
    let encoded = request.uri().path().strip_prefix('/').unwrap_or_default();
    let Ok(decoded) = percent_encoding::percent_decode_str(encoded).decode_utf8() else {
        responder.respond(error_response(StatusCode::BAD_REQUEST));
        return;
    };
    let Ok(path) = tauri::path::SafePathBuf::new(decoded.as_ref().into()) else {
        responder.respond(error_response(StatusCode::FORBIDDEN));
        return;
    };
    if !app.asset_protocol_scope().is_allowed(&path) {
        responder.respond(error_response(StatusCode::FORBIDDEN));
        return;
    }
    tauri::async_runtime::spawn(async move {
        let response = match read_image(path.as_ref()).await {
            Ok((bytes, mime)) => {
                let len = bytes.len();
                Response::builder()
                    .header("Access-Control-Allow-Origin", origin)
                    .header("Content-Type", mime)
                    .header("Content-Length", len)
                    .header("Cache-Control", "no-store")
                    .header("X-Content-Type-Options", "nosniff")
                    .header("Content-Security-Policy", "default-src 'none'; sandbox")
                    .body(if request.method() == Method::HEAD {
                        Vec::new()
                    } else {
                        bytes
                    })
                    .unwrap()
            }
            Err(status) => error_response(status),
        };
        responder.respond(response);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn third_party_tabs_have_no_asset_access_even_at_a_local_url() {
        for page in [
            "https://claude.ai",
            "tauri://localhost",
            "http://localhost:1420",
        ] {
            for label in ["ai-chatbot-1", "unknown"] {
                assert!(asset_origin(label, &Url::parse(page).unwrap(), None).is_none());
            }
        }
        assert!(asset_origin("main", &Url::parse("https://claude.ai").unwrap(), None).is_none());
    }

    #[test]
    fn app_previews_use_the_exact_platform_origin() {
        for page in [
            "tauri://localhost",
            "http://tauri.localhost",
            "https://tauri.localhost",
        ] {
            assert_eq!(
                asset_origin("main", &Url::parse(page).unwrap(), None).as_deref(),
                Some(page)
            );
        }
        let dev = Url::parse("http://localhost:1420").unwrap();
        assert_eq!(
            asset_origin("main", &dev, Some(&dev)).as_deref(),
            Some("http://localhost:1420")
        );
        assert!(asset_origin("main", &dev, None).is_none());
    }

    #[test]
    fn image_previews_reject_non_images_and_oversized_files() {
        let root = std::env::temp_dir().join(format!("hopper-assets-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let image = root.join("image with spaces.png");
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";
        std::fs::write(&image, png).unwrap();
        let text = root.join("credentials.json");
        std::fs::write(&text, b"{\"token\":\"private\"}").unwrap();
        let large = root.join("large.png");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(MAX_ASSET_BYTES + 1)
            .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (bytes, mime) = read_image(&image).await.unwrap();
            assert_eq!(bytes, png);
            assert_eq!(mime, "image/png");
            assert_eq!(
                read_image(&text).await.unwrap_err(),
                StatusCode::UNSUPPORTED_MEDIA_TYPE
            );
            assert_eq!(
                read_image(&large).await.unwrap_err(),
                StatusCode::PAYLOAD_TOO_LARGE
            );
            assert_eq!(read_image(&root).await.unwrap_err(), StatusCode::FORBIDDEN);
        });
        std::fs::remove_dir_all(root).unwrap();
    }
}
