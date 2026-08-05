use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

mod settings;

#[cfg(target_os = "macos")]
mod shake_shelf;

#[cfg(target_os = "windows")]
mod windows_shelf;

use settings::{AppSettings, SettingsStore};

const AUTOSTART_ARG: &str = "--autostart";
const TRAY_OPEN: &str = "open";
const TRAY_SHOW_SHELF: &str = "show-shelf";
const TRAY_QUIT: &str = "quit";

struct AppState {
    path: PathBuf,
    next_item_id: u64,
    shelf_items: Vec<ShelfItem>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformCapabilities {
    platform: String,
    shake_supported: bool,
    native_file_drag_supported: bool,
    accessibility_required: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShelfItem {
    id: u64,
    path: String,
    content: Option<String>,
    name: String,
    kind: ShelfItemKind,
    size: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum ShelfItemKind {
    File,
    Directory,
    Text,
    Other,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct PersistedShelf {
    next_item_id: u64,
    items: Vec<ShelfItem>,
}

impl AppState {
    fn load(app: &tauri::AppHandle) -> Result<Self, String> {
        let path = app
            .path()
            .app_config_dir()
            .map_err(|error| error.to_string())?
            .join("shelf.json");
        let persisted = match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => PersistedShelf::default(),
            Err(error) => return Err(error.to_string()),
        };
        let max_item_id = persisted
            .items
            .iter()
            .map(|item| item.id)
            .max()
            .unwrap_or(0);
        Ok(Self {
            path,
            next_item_id: persisted.next_item_id.max(max_item_id),
            shelf_items: persisted.items,
        })
    }

    fn save(&self) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "shelf path has no parent directory".to_string())?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary_path = self.path.with_extension("json.tmp");
        let persisted = PersistedShelf {
            next_item_id: self.next_item_id,
            items: self.shelf_items.clone(),
        };
        let contents = serde_json::to_vec_pretty(&persisted).map_err(|error| error.to_string())?;
        std::fs::write(&temporary_path, contents).map_err(|error| error.to_string())?;
        std::fs::rename(&temporary_path, &self.path).map_err(|error| error.to_string())
    }
}

fn update_shelf<F>(app: &tauri::AppHandle, update: F) -> Result<Vec<ShelfItem>, String>
where
    F: FnOnce(&mut AppState),
{
    let state = app.state::<Mutex<AppState>>();
    let mut state = state.lock().map_err(|_| "failed to lock app state")?;
    let previous_id = state.next_item_id;
    let previous_items = state.shelf_items.clone();
    update(&mut state);
    if let Err(error) = state.save() {
        state.next_item_id = previous_id;
        state.shelf_items = previous_items;
        return Err(error);
    }
    let items = state.shelf_items.clone();
    app.emit("shelf-changed", &items)
        .map_err(|error| error.to_string())?;
    Ok(items)
}

#[tauri::command]
fn list_shelf_items(state: tauri::State<'_, Mutex<AppState>>) -> Result<Vec<ShelfItem>, String> {
    let state = state.lock().map_err(|_| "failed to lock app state")?;
    Ok(state.shelf_items.clone())
}

#[tauri::command]
fn add_shelf_paths(paths: Vec<String>, app: tauri::AppHandle) -> Result<Vec<ShelfItem>, String> {
    update_shelf(&app, move |state| {
        for path in paths {
            if path.trim().is_empty() || state.shelf_items.iter().any(|item| item.path == path) {
                continue;
            }

            state.next_item_id += 1;
            let id = state.next_item_id;
            state.shelf_items.push(build_shelf_item(id, path));
        }
    })
}

#[tauri::command]
fn add_shelf_text(text: String, app: tauri::AppHandle) -> Result<Vec<ShelfItem>, String> {
    add_shelf_text_to_app(&app, text)
}

fn add_shelf_text_to_app(app: &tauri::AppHandle, text: String) -> Result<Vec<ShelfItem>, String> {
    if text.trim().is_empty() {
        return Err("text is empty".to_string());
    }

    update_shelf(app, move |state| {
        if !state
            .shelf_items
            .iter()
            .any(|item| item.content.as_deref() == Some(text.as_str()))
        {
            state.next_item_id += 1;
            let id = state.next_item_id;
            state.shelf_items.push(build_text_shelf_item(id, text));
        }
    })
}

#[tauri::command]
fn remove_shelf_item(id: u64, app: tauri::AppHandle) -> Result<Vec<ShelfItem>, String> {
    update_shelf(&app, move |state| {
        state.shelf_items.retain(|item| item.id != id);
    })
}

#[tauri::command]
fn clear_shelf(app: tauri::AppHandle) -> Result<Vec<ShelfItem>, String> {
    update_shelf(&app, |state| state.shelf_items.clear())
}

#[tauri::command]
fn platform_capabilities() -> PlatformCapabilities {
    #[cfg(target_os = "macos")]
    return PlatformCapabilities {
        platform: "macos".to_string(),
        shake_supported: true,
        native_file_drag_supported: true,
        accessibility_required: true,
    };

    #[cfg(target_os = "windows")]
    return PlatformCapabilities {
        platform: "windows".to_string(),
        shake_supported: false,
        native_file_drag_supported: false,
        accessibility_required: false,
    };

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    PlatformCapabilities {
        platform: std::env::consts::OS.to_string(),
        shake_supported: false,
        native_file_drag_supported: false,
        accessibility_required: false,
    }
}

#[tauri::command]
fn autostart_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_autostart(enabled: bool, app: tauri::AppHandle) -> Result<bool, String> {
    let autostart = app.autolaunch();
    if enabled {
        autostart.enable().map_err(|error| error.to_string())?;
    } else {
        autostart.disable().map_err(|error| error.to_string())?;
    }
    autostart.is_enabled().map_err(|error| error.to_string())
}

#[tauri::command]
fn app_settings(state: tauri::State<'_, Mutex<SettingsStore>>) -> Result<AppSettings, String> {
    let state = state.lock().map_err(|_| "failed to lock settings")?;
    Ok(state.settings())
}

#[tauri::command]
fn set_shake_enabled(
    enabled: bool,
    state: tauri::State<'_, Mutex<SettingsStore>>,
    app: tauri::AppHandle,
) -> Result<AppSettings, String> {
    let settings = state
        .lock()
        .map_err(|_| "failed to lock settings")?
        .set_shake_enabled(enabled)?;
    #[cfg(target_os = "macos")]
    shake_shelf::set_enabled(&app, settings.shake_enabled);
    Ok(settings)
}

#[tauri::command]
fn set_shake_sensitivity(
    sensitivity: u8,
    state: tauri::State<'_, Mutex<SettingsStore>>,
) -> Result<AppSettings, String> {
    let settings = state
        .lock()
        .map_err(|_| "failed to lock settings")?
        .set_shake_sensitivity(sensitivity)?;
    #[cfg(target_os = "macos")]
    shake_shelf::set_sensitivity(settings.shake_sensitivity);
    Ok(settings)
}

#[tauri::command]
fn accessibility_permission_status() -> bool {
    #[cfg(target_os = "macos")]
    return unsafe { AXIsProcessTrusted() };

    #[cfg(not(target_os = "macos"))]
    false
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn()
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    Err("accessibility settings are available only on macOS".to_string())
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
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

    #[cfg(target_os = "windows")]
    return windows_shelf::show_for_test(&app);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err("shake shelf is currently available only on macOS".to_string())
}

#[tauri::command]
fn hide_shake_shelf(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return shake_shelf::hide(&app);

    #[cfg(target_os = "windows")]
    return windows_shelf::hide(&app);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err("shake shelf is currently available only on macOS".to_string())
}

#[tauri::command]
fn start_shake_shelf_drag(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return shake_shelf::start_dragging(&app);

    #[cfg(target_os = "windows")]
    return windows_shelf::start_dragging(&app);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err("shake shelf is currently available only on macOS".to_string())
}

#[tauri::command]
fn begin_native_file_drag(path: String, app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return shake_shelf::begin_file_drag(&app, path);

    #[cfg(target_os = "windows")]
    return windows_shelf::begin_file_drag(&app, path);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err("native file drag is currently available only on macOS".to_string())
}

#[tauri::command]
fn open_shelf_path(path: String) -> Result<(), String> {
    run_platform_open(&path, false)
}

#[tauri::command]
fn reveal_shelf_path(path: String) -> Result<(), String> {
    run_platform_open(&path, true)
}

fn run_platform_open(path: &str, reveal: bool) -> Result<(), String> {
    if path.trim().is_empty() || !Path::new(path).exists() {
        return Err("the shelf item no longer exists".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("open");
        if reveal {
            command.arg("-R");
        }
        command
            .arg(path)
            .spawn()
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let mut command = if reveal {
            let mut command = std::process::Command::new("explorer.exe");
            command.arg("/select,");
            command
        } else {
            let mut command = std::process::Command::new("cmd");
            command.args(["/C", "start", ""]);
            command
        };
        command
            .arg(path)
            .spawn()
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err("opening shelf items is currently available only on macOS".to_string())
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
        content: None,
        name,
        kind,
        size: metadata
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len()),
    }
}

fn build_text_shelf_item(id: u64, content: String) -> ShelfItem {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut name = normalized.chars().take(80).collect::<String>();
    if normalized.chars().count() > 80 {
        name.push_str("...");
    }
    let size = content.len() as u64;

    ShelfItem {
        id,
        path: String::new(),
        content: Some(content),
        name,
        kind: ShelfItemKind::Text,
        size: Some(size),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_unicode_text_preview_without_losing_content() {
        let content = "选中文本 ".repeat(30);
        let item = build_text_shelf_item(1, content.clone());

        assert!(matches!(item.kind, ShelfItemKind::Text));
        assert_eq!(item.content.as_deref(), Some(content.as_str()));
        assert!(item.name.ends_with("..."));
        assert_eq!(item.name.trim_end_matches("...").chars().count(), 80);
    }

    #[test]
    fn persisted_shelf_round_trips_text_items() {
        let persisted = PersistedShelf {
            next_item_id: 7,
            items: vec![build_text_shelf_item(7, "saved text".to_string())],
        };

        let encoded = serde_json::to_string(&persisted).unwrap();
        let decoded: PersistedShelf = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.next_item_id, 7);
        assert_eq!(decoded.items[0].content.as_deref(), Some("saved text"));
        assert!(matches!(decoded.items[0].kind, ShelfItemKind::Text));
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, TRAY_OPEN, "Open DropAir", true, None::<&str>)?;
    let show_shelf = MenuItem::with_id(app, TRAY_SHOW_SHELF, "Show Shelf", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, TRAY_QUIT, "Quit DropAir", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &show_shelf, &separator, &quit])?;

    let mut tray = TrayIconBuilder::with_id("dropair-tray")
        .tooltip("DropAir")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN => show_main_window(app),
            TRAY_SHOW_SHELF => {
                #[cfg(target_os = "macos")]
                let _ = shake_shelf::show_for_test(app);
                #[cfg(target_os = "windows")]
                let _ = windows_shelf::show_for_test(app);
            }
            TRAY_QUIT => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone()).icon_as_template(true);
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let launched_from_autostart = std::env::args().any(|arg| arg == AUTOSTART_ARG);
    let shortcut_plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                #[cfg(target_os = "macos")]
                let _ = shake_shelf::toggle(app);

                #[cfg(target_os = "windows")]
                let _ = windows_shelf::toggle(app);

                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                show_main_window(app);
            }
        })
        .build();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|arg| arg == AUTOSTART_ARG) {
                show_main_window(app);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .plugin(shortcut_plugin)
        .setup(|app| {
            let app_state = AppState::load(app.handle()).map_err(std::io::Error::other)?;
            app.manage(Mutex::new(app_state));
            if let Err(error) = app
                .global_shortcut()
                .register("CommandOrControl+Shift+Space")
            {
                eprintln!("failed to register DropAir global shortcut: {error}");
            }
            let settings_store =
                SettingsStore::load(app.handle()).map_err(std::io::Error::other)?;
            let settings = settings_store.settings();
            app.manage(Mutex::new(settings_store));
            #[cfg(target_os = "macos")]
            shake_shelf::setup(app.handle(), &settings)?;
            #[cfg(target_os = "windows")]
            windows_shelf::setup(app.handle()).map_err(std::io::Error::other)?;
            setup_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_shelf_items,
            add_shelf_paths,
            add_shelf_text,
            remove_shelf_item,
            clear_shelf,
            platform_capabilities,
            autostart_enabled,
            set_autostart,
            app_settings,
            set_shake_enabled,
            set_shake_sensitivity,
            accessibility_permission_status,
            open_accessibility_settings,
            shake_monitor_status,
            shake_monitor_diagnostics,
            show_shake_shelf_for_test,
            hide_shake_shelf,
            start_shake_shelf_drag,
            begin_native_file_drag,
            open_shelf_path,
            reveal_shelf_path
        ])
        .build(tauri::generate_context!())
        .expect("error while building DropAir");

    app.run(move |app, event| {
        if matches!(event, tauri::RunEvent::Ready) && !launched_from_autostart {
            show_main_window(app);
        }

        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            show_main_window(app);
        }
    });
}
