//! Desktop-only lifecycle and placement of the embedded sidebar browser.
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, Url, WebviewUrl};

use crate::web_chat;

/// Emitted when a download from an assistant tab finishes, so Hopper can offer
/// to send that file to the chat.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebChatDownload {
    webview_label: String,
    path: String,
    file_name: String,
}

type RequestedDownloads = Arc<Mutex<HashMap<String, PathBuf>>>;

/// Where a finished download landed. Some platforms only report the
/// destination when the download is requested, so fall back to that.
fn finished_download(
    requested: &RequestedDownloads,
    webview_label: &str,
    url: &str,
    completed_path: Option<PathBuf>,
    success: bool,
) -> Option<WebChatDownload> {
    let requested_path = requested.lock().ok().and_then(|mut paths| paths.remove(url));
    if !success {
        return None;
    }
    let path = completed_path.or(requested_path)?;
    let file_name = path.file_name()?.to_string_lossy().into_owned();
    Some(WebChatDownload {
        webview_label: webview_label.to_string(),
        path: path.to_string_lossy().into_owned(),
        file_name,
    })
}

#[tauri::command]
pub(crate) async fn create_sidebar_browser(
    app: tauri::AppHandle,
    label: String,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if !label.starts_with("ai-chatbot-")
        || ![x, y, width, height].iter().all(|value| value.is_finite())
        || width <= 0.0
        || height <= 0.0
    {
        return Err("Invalid sidebar browser configuration".into());
    }

    let url = Url::parse(&url).map_err(|error| error.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Sidebar browser URL must use HTTP or HTTPS".into());
    }

    let window = app.get_window("main").ok_or("Main window not found")?;
    let browser_data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("sidebar-browser");
    let popup_app = app.clone();
    let popup_label = label.clone();
    let download_app = app.clone();
    let requested_downloads: RequestedDownloads = Arc::new(Mutex::new(HashMap::new()));
    let builder = tauri::webview::WebviewBuilder::new(label, WebviewUrl::External(url))
        .data_directory(browser_data_directory)
        .enable_clipboard_access()
        .initialization_script(web_chat::WEB_CHAT_SCRIPT)
        // Assistant sites handle their own file drops (uploads). Hopper's native
        // handler would swallow them and report positions in the wrong frame.
        .disable_drag_drop_handler()
        .on_new_window(move |url, _features| {
            if let Some(webview) = popup_app.get_webview(&popup_label) {
                let _ = webview.navigate(url);
            }
            tauri::webview::NewWindowResponse::Deny
        })
        // Downloads save to the system's default download folder; finished
        // ones are offered to the Hopper chat.
        .on_download(move |webview, event| {
            match event {
                tauri::webview::DownloadEvent::Requested { url, destination } => {
                    if let Ok(mut paths) = requested_downloads.lock() {
                        paths.insert(url.to_string(), destination.clone());
                    }
                }
                tauri::webview::DownloadEvent::Finished { url, path, success } => {
                    if let Some(download) = finished_download(
                        &requested_downloads,
                        webview.label(),
                        url.as_str(),
                        path,
                        success,
                    ) {
                        let _ = download_app.emit("web-chat-download-finished", download);
                    }
                }
                _ => {}
            }
            true
        });

    window
        .add_child(
            builder,
            tauri::LogicalPosition::new(x, y),
            tauri::LogicalSize::new(width, height),
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_finished_downloads_with_requested_destination_fallback() {
        let requested: RequestedDownloads = Arc::new(Mutex::new(HashMap::new()));
        let url = "blob:https://claude.ai/abc";
        requested
            .lock()
            .unwrap()
            .insert(url.into(), PathBuf::from("/home/me/Downloads/report.pdf"));
        let download = finished_download(&requested, "ai-chatbot-tab-1", url, None, true).unwrap();
        assert_eq!(download.path, "/home/me/Downloads/report.pdf");
        assert_eq!(download.file_name, "report.pdf");
        assert_eq!(download.webview_label, "ai-chatbot-tab-1");
        assert!(requested.lock().unwrap().is_empty());

        let finished = finished_download(
            &requested,
            "ai-chatbot-tab-1",
            "https://x/y",
            Some(PathBuf::from("/tmp/data.csv")),
            true,
        )
        .unwrap();
        assert_eq!(finished.file_name, "data.csv");
    }

    #[test]
    fn ignores_failed_or_unlocated_downloads() {
        let requested: RequestedDownloads = Arc::new(Mutex::new(HashMap::new()));
        requested.lock().unwrap().insert("u".into(), PathBuf::from("/tmp/a.pdf"));
        assert!(finished_download(&requested, "tab", "u", None, false).is_none());
        assert!(requested.lock().unwrap().is_empty());
        assert!(finished_download(&requested, "tab", "missing", None, true).is_none());
    }
}

#[tauri::command]
pub(crate) async fn set_sidebar_browser_bounds(
    app: tauri::AppHandle,
    label: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if !label.starts_with("ai-chatbot-")
        || ![x, y, width, height].iter().all(|value| value.is_finite())
        || width <= 0.0
        || height <= 0.0
    {
        return Err("Invalid sidebar browser bounds".into());
    }
    let view = app.get_webview(&label).ok_or("Sidebar browser not found")?;
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        let (tx, rx) = tokio::sync::oneshot::channel();
        view.with_webview(move |platform| {
            let result = (|| -> Result<(), String> {
                let browser = platform.inner();
                // WebKitGTK exposes this native preference to websites through
                // prefers-color-scheme, allowing their own dark themes to render.
                if let Some(settings) = browser.settings() {
                    settings.set_gtk_application_prefer_dark_theme(true);
                }
                let parent = browser.parent().ok_or("Browser has no native parent")?;
                let fixed = if let Ok(fixed) = parent.clone().downcast::<gtk::Fixed>() {
                    fixed
                } else {
                    let vbox = parent
                        .downcast::<gtk::Box>()
                        .map_err(|_| "Unsupported native browser container")?;
                    vbox.remove(&browser);
                    // Keep the main application as the base layer, with the sidebar
                    // browser in an explicitly positioned overlay.
                    let overlay = vbox
                        .children()
                        .into_iter()
                        .find_map(|child| child.downcast::<gtk::Overlay>().ok())
                        .unwrap_or_else(|| {
                            let overlay = gtk::Overlay::new();
                            let base = gtk::Box::new(gtk::Orientation::Vertical, 0);
                            for child in vbox.children() {
                                vbox.remove(&child);
                                base.pack_start(&child, true, true, 0);
                            }
                            overlay.add(&base);
                            vbox.pack_start(&overlay, true, true, 0);
                            overlay.show_all();
                            overlay
                        });
                    let fixed = gtk::Fixed::new();
                    // The overlay's input window must cover only the browser.
                    // A fill-aligned overlay intercepts clicks across Hopper,
                    // including the composer and the sidebar resize divider.
                    fixed.set_halign(gtk::Align::Start);
                    fixed.set_valign(gtk::Align::Start);
                    overlay.add_overlay(&fixed);
                    fixed.put(&browser, 0, 0);
                    let fixed_for_close = fixed.downgrade();
                    browser.connect_destroy(move |_| {
                        if let Some(fixed) = fixed_for_close.upgrade() {
                            if let Some(parent) = fixed
                                .parent()
                                .and_then(|parent| parent.downcast::<gtk::Container>().ok())
                            {
                                parent.remove(&fixed);
                            }
                        }
                    });
                    fixed.show_all();
                    fixed
                };
                let (x, y, width, height) = (
                    x.round() as i32,
                    y.round() as i32,
                    width.round().max(1.0) as i32,
                    height.round().max(1.0) as i32,
                );
                fixed.set_margin_start(x.max(0));
                fixed.set_margin_top(y.max(0));
                fixed.set_size_request(width, height);
                fixed.move_(&browser, 0, 0);
                browser.set_size_request(width, height);
                // Let GTK allocate the child relative to the bounded overlay.
                // Window-relative allocation here would offset it twice.
                fixed.queue_resize();
                Ok(())
            })();
            let _ = tx.send(result);
        })
        .map_err(|error| error.to_string())?;
        rx.await.map_err(|error| error.to_string())?
    }
    #[cfg(not(target_os = "linux"))]
    {
        view.set_position(tauri::LogicalPosition::new(x, y))
            .map_err(|error| error.to_string())?;
        view.set_size(tauri::LogicalSize::new(width, height))
            .map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub(crate) async fn set_sidebar_browser_visible(
    app: tauri::AppHandle,
    label: String,
    visible: bool,
) -> Result<(), String> {
    if !label.starts_with("ai-chatbot-") {
        return Err("Invalid sidebar browser label".into());
    }
    let view = app.get_webview(&label).ok_or("Sidebar browser not found")?;
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        let (tx, rx) = tokio::sync::oneshot::channel();
        view.with_webview(move |platform| {
            let browser = platform.inner();
            // Hiding only the browser widget leaves the gtk::Fixed container
            // visible; it still intercepts mouse events. Hide/show the Fixed
            // (browser's direct parent) to fully remove the input surface.
            let fixed = browser
                .parent()
                .and_then(|p| p.downcast::<gtk::Fixed>().ok());
            if let Some(fixed) = fixed {
                if visible {
                    fixed.show_all();
                } else {
                    fixed.hide();
                }
            } else {
                if visible {
                    browser.show();
                } else {
                    browser.hide();
                }
            }
            let _ = tx.send(());
        })
        .map_err(|error| error.to_string())?;
        rx.await.map_err(|error| error.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        if visible {
            view.show().map_err(|error| error.to_string())
        } else {
            view.hide().map_err(|error| error.to_string())
        }
    }
}

#[tauri::command]
pub(crate) async fn reload_sidebar_browser(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    if !label.starts_with("ai-chatbot-") {
        return Err("Invalid sidebar browser label".into());
    }
    let view = app.get_webview(&label).ok_or("Sidebar browser not found")?;
    view.eval("window.location.reload()")
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn hide_sidebar_browsers(app: tauri::AppHandle) -> Result<(), String> {
    let labels = app
        .webviews()
        .into_keys()
        .filter(|label| label.starts_with("ai-chatbot-"))
        .collect::<Vec<_>>();
    for label in labels {
        set_sidebar_browser_visible(app.clone(), label, false).await?;
    }
    Ok(())
}
