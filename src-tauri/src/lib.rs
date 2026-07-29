use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use tauri::Emitter;

#[cfg(target_os = "macos")]
mod shake_shelf;

#[derive(Default)]
struct AppState {
    next_item_id: u64,
    shelf_items: Vec<ShelfItem>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShelfItem {
    id: u64,
    path: String,
    name: String,
    kind: ShelfItemKind,
    size: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
enum ShelfItemKind {
    File,
    Directory,
    Other,
}

#[tauri::command]
fn list_shelf_items(state: tauri::State<'_, Mutex<AppState>>) -> Result<Vec<ShelfItem>, String> {
    let state = state.lock().map_err(|_| "failed to lock app state")?;
    Ok(state.shelf_items.clone())
}

#[tauri::command]
fn add_shelf_paths(
    paths: Vec<String>,
    state: tauri::State<'_, Mutex<AppState>>,
    app: tauri::AppHandle,
) -> Result<Vec<ShelfItem>, String> {
    let mut state = state.lock().map_err(|_| "failed to lock app state")?;

    for path in paths {
        if path.trim().is_empty() || state.shelf_items.iter().any(|item| item.path == path) {
            continue;
        }

        state.next_item_id += 1;
        let id = state.next_item_id;
        state.shelf_items.push(build_shelf_item(id, path));
    }

    let items = state.shelf_items.clone();
    app.emit("shelf-changed", &items)
        .map_err(|error| error.to_string())?;
    Ok(items)
}

#[tauri::command]
fn remove_shelf_item(
    id: u64,
    state: tauri::State<'_, Mutex<AppState>>,
    app: tauri::AppHandle,
) -> Result<Vec<ShelfItem>, String> {
    let mut state = state.lock().map_err(|_| "failed to lock app state")?;
    state.shelf_items.retain(|item| item.id != id);
    let items = state.shelf_items.clone();
    app.emit("shelf-changed", &items)
        .map_err(|error| error.to_string())?;
    Ok(items)
}

#[tauri::command]
fn clear_shelf(
    state: tauri::State<'_, Mutex<AppState>>,
    app: tauri::AppHandle,
) -> Result<Vec<ShelfItem>, String> {
    let mut state = state.lock().map_err(|_| "failed to lock app state")?;
    state.shelf_items.clear();
    let items = state.shelf_items.clone();
    app.emit("shelf-changed", &items)
        .map_err(|error| error.to_string())?;
    Ok(items)
}

#[tauri::command]
fn shake_monitor_status() -> String {
    #[cfg(target_os = "macos")]
    return shake_shelf::monitor_status().to_string();

    #[cfg(not(target_os = "macos"))]
    "unsupported".to_string()
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn shake_monitor_diagnostics() -> shake_shelf::ShakeDiagnostics {
    shake_shelf::diagnostics()
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
fn shake_monitor_diagnostics() -> serde_json::Value {
    serde_json::json!({
        "mouseDowns": 0,
        "motionSamples": 0,
        "maxDirectionChanges": 0,
        "triggers": 0
    })
}

#[tauri::command]
fn show_shake_shelf_for_test(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return shake_shelf::show_for_test(&app);

    #[cfg(not(target_os = "macos"))]
    Err("shake shelf is currently available only on macOS".to_string())
}

fn build_shelf_item(id: u64, path: String) -> ShelfItem {
    let path_ref = Path::new(&path);
    let metadata = std::fs::metadata(path_ref).ok();
    let kind = metadata
        .as_ref()
        .map(|metadata| {
            if metadata.is_file() {
                ShelfItemKind::File
            } else if metadata.is_dir() {
                ShelfItemKind::Directory
            } else {
                ShelfItemKind::Other
            }
        })
        .unwrap_or(ShelfItemKind::Other);

    let name = path_ref
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(path.as_str())
        .to_string();

    ShelfItem {
        id,
        path,
        name,
        kind,
        size: metadata
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(AppState::default()))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            shake_shelf::setup(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_shelf_items,
            add_shelf_paths,
            remove_shelf_item,
            clear_shelf,
            shake_monitor_status,
            shake_monitor_diagnostics,
            show_shake_shelf_for_test
        ])
        .run(tauri::generate_context!())
        .expect("error while running DropAir");
}
