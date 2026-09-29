//! Desktop-only lifecycle and placement of the embedded sidebar browser.
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, Url, WebviewUrl};

use crate::shared::bridge_core;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SidebarBrowserDownload {
    webview_label: String,
    url: String,
    path: Option<String>,
    file_name: Option<String>,
    success: bool,
    imported: bool,
    error: Option<String>,
}

type RequestedDownloads = Arc<Mutex<HashMap<String, PathBuf>>>;

fn remember_download_destination(
    requested_downloads: &RequestedDownloads,
    url: &str,
    destination: PathBuf,
) {
    if let Ok(mut downloads) = requested_downloads.lock() {
        downloads.insert(url.to_string(), destination);
    }
}

fn resolve_download_path(
    requested_downloads: &RequestedDownloads,
    url: &str,
    completed_path: Option<PathBuf>,
) -> Option<PathBuf> {
    let requested_path = requested_downloads
        .lock()
        .ok()
        .and_then(|mut downloads| downloads.remove(url));
    completed_path.or(requested_path)
}

fn import_finished_download(
    app: tauri::AppHandle,
    app_data_directory: PathBuf,
    webview_label: String,
    url: Url,
    path: Option<PathBuf>,
    success: bool,
) {
    let file_name = path
        .as_deref()
        .and_then(|path| path.file_name())
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .or_else(|| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
    let url = url.to_string();
    let path = path.map(|value| value.to_string_lossy().to_string());
    let import_path = path.clone();
    let emit_result = move |imported: bool, error: Option<String>| {
        let _ = app.emit(
            "sidebar-browser-download",
            SidebarBrowserDownload {
                webview_label,
                url,
                path,
                file_name,
                success,
                imported,
                error,
            },
        );
    };
    if success {
        if let Some(import_path) = import_path {
            tauri::async_runtime::spawn(async move {
                match bridge_core::bridge_import_file_core(&app_data_directory, import_path).await {
                    Ok(_) => emit_result(true, None),
                    Err(error) => emit_result(false, Some(error)),
                }
            });
        } else {
            emit_result(
                false,
                Some(
                    "Hopper could not locate the downloaded file. Use Import file in Bridge Inbox."
                        .to_string(),
                ),
            );
        }
    } else {
        emit_result(false, Some("The download did not complete.".to_string()));
    }
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
    let app_data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let browser_data_directory = app_data_directory.join("sidebar-browser");
    let popup_app = app.clone();
    let popup_label = label.clone();
    let download_app = app.clone();
    let requested_downloads = Arc::new(Mutex::new(HashMap::<String, PathBuf>::new()));
    let builder = tauri::webview::WebviewBuilder::new(label, WebviewUrl::External(url))
        .data_directory(browser_data_directory)
        .enable_clipboard_access()
        .on_new_window(move |url, _features| {
            if let Some(webview) = popup_app.get_webview(&popup_label) {
                let _ = webview.navigate(url);
            }
            tauri::webview::NewWindowResponse::Deny
        })
        .on_download(move |webview, event| {
            match event {
                tauri::webview::DownloadEvent::Requested { url, destination } => {
                    remember_download_destination(
                        &requested_downloads,
                        url.as_str(),
                        destination.clone(),
                    );
                }
                tauri::webview::DownloadEvent::Finished { url, path, success } => {
                    let path = resolve_download_path(&requested_downloads, url.as_str(), path);
                    import_finished_download(
                        download_app.clone(),
                        app_data_directory.clone(),
                        webview.label().to_string(),
                        url,
                        path,
                        success,
                    );
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
    use super::{remember_download_destination, resolve_download_path, RequestedDownloads};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[test]
    fn resolves_successful_download_from_requested_destination_when_finished_path_is_missing() {
        let downloads: RequestedDownloads = Arc::new(Mutex::new(HashMap::new()));
        let url = "blob:https://chatgpt.com/generated-file";
        let destination = PathBuf::from("/tmp/generated-design.docx");

        remember_download_destination(&downloads, url, destination.clone());

        assert_eq!(
            resolve_download_path(&downloads, url, None),
            Some(destination)
        );
        assert!(downloads.lock().expect("downloads lock").is_empty());
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
