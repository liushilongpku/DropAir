use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const SHELF_LABEL: &str = "shake-shelf";

fn get_or_create(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(SHELF_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        SHELF_LABEL,
        WebviewUrl::App("index.html?shelf=1".into()),
    )
    .title("DropAir Shelf")
    .inner_size(320.0, 240.0)
    .min_inner_size(180.0, 130.0)
    .decorations(false)
    .resizable(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .build()
    .map_err(|error| error.to_string())
}

pub fn setup(app: &AppHandle) -> Result<(), String> {
    let _ = get_or_create(app)?;
    Ok(())
}

pub fn show_for_test(app: &AppHandle) -> Result<(), String> {
    let window = get_or_create(app)?;
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

pub fn hide(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(SHELF_LABEL) {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn toggle(app: &AppHandle) -> Result<(), String> {
    let window = get_or_create(app)?;
    if window.is_visible().map_err(|error| error.to_string())? {
        window.hide().map_err(|error| error.to_string())
    } else {
        show_for_test(app)
    }
}

pub fn start_dragging(app: &AppHandle) -> Result<(), String> {
    let window = get_or_create(app)?;
    window.start_dragging().map_err(|error| error.to_string())
}

pub fn begin_file_drag(_app: &AppHandle, _path: String) -> Result<(), String> {
    Err("Windows uses the Shelf item's URI drag gesture for file drag-out".to_string())
}
