//! Desktop-only lifecycle and placement of the embedded sidebar browser.
use tauri::{Manager, Url, WebviewUrl};

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
    let data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("sidebar-browser");
    let popup_app = app.clone();
    let popup_label = label.clone();
    let builder = tauri::webview::WebviewBuilder::new(label, WebviewUrl::External(url))
        .data_directory(data_directory)
        .enable_clipboard_access()
        .on_new_window(move |url, _features| {
            if let Some(webview) = popup_app.get_webview(&popup_label) {
                let _ = webview.navigate(url);
            }
            tauri::webview::NewWindowResponse::Deny
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
                if visible { fixed.show_all(); } else { fixed.hide(); }
            } else {
                if visible { browser.show(); } else { browser.hide(); }
            }
            let _ = tx.send(());
        })
        .map_err(|error| error.to_string())?;
        rx.await.map_err(|error| error.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        // On macOS and Windows the JS webview.hide()/show() path is reliable;
        // this command is a no-op so the frontend can call it unconditionally.
        let _ = (view, visible);
        Ok(())
    }
}
