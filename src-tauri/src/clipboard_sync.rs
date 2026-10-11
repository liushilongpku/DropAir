use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(target_os = "windows")]
use std::ptr::null_mut;

use crate::settings::SettingsStore;
use crate::transfer;

/// Images larger than this are not synchronized.
pub const MAX_CLIPBOARD_IMAGE_BYTES: u64 = 24 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(700);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardMode {
    /// No clipboard synchronization.
    Off,
    /// Send only from the "Send clipboard" button; remote content overwrites the clipboard.
    Manual,
    /// Automatically send local copies; remote content is ignored.
    Push,
    /// Automatically send local copies and apply remote content to the clipboard.
    Sync,
}

impl ClipboardMode {
    pub fn parse(value: &str) -> Self {
        match value {
            "manual" => Self::Manual,
            "push" => Self::Push,
            "sync" => Self::Sync,
            _ => Self::Off,
        }
    }

    pub fn auto_sends(self) -> bool {
        matches!(self, Self::Push | Self::Sync)
    }

    pub fn accepts(self) -> bool {
        matches!(self, Self::Manual | Self::Sync)
    }
}

#[derive(Clone)]
pub enum ClipboardPayload {
    Text(String),
    Image { width: u32, height: u32, rgba: Vec<u8> },
    Files(Vec<ClipboardFile>),
}

#[derive(Clone)]
pub struct ClipboardFile {
    pub path: PathBuf,
    pub name: String,
}

/// Sequence of the clipboard change we last observed or wrote, so our own writes
/// are not echoed back to the peer that just sent them.
static LAST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn current_mode(app: &AppHandle) -> ClipboardMode {
    app.state::<Mutex<SettingsStore>>()
        .lock()
        .map(|store| ClipboardMode::parse(&store.settings().clipboard_mode))
        .unwrap_or(ClipboardMode::Off)
}

pub fn watch(app: AppHandle) {
    let mut retry_after = Instant::now();
    if let Some(sequence) = clipboard_sequence() {
        LAST_SEQUENCE.store(sequence, Ordering::Relaxed);
    }
    loop {
        thread::sleep(POLL_INTERVAL);
        let mode = current_mode(&app);
        let Some(sequence) = clipboard_sequence() else {
            continue;
        };
        if sequence == LAST_SEQUENCE.load(Ordering::Relaxed) {
            continue;
        }
        if Instant::now() < retry_after {
            continue;
        }
        // Remember the change even when we are not sending, so enabling a mode
        // does not replay history.
        if !mode.auto_sends() {
            LAST_SEQUENCE.store(sequence, Ordering::Relaxed);
            continue;
        }
        if !transfer::has_linked_peers(&app) {
            continue;
        }
        let Some(payload) = read_clipboard(&app) else {
            LAST_SEQUENCE.store(sequence, Ordering::Relaxed);
            continue;
        };
        let description = describe(&payload);
        match transfer::send_clipboard_payload(&app, &payload) {
            Ok(count) => {
                LAST_SEQUENCE.store(sequence, Ordering::Relaxed);
                retry_after = Instant::now();
                let _ = app.emit(
                    "transfer-status",
                    serde_json::json!({
                        "peerId": "clipboard",
                        "state": "sent",
                        "message": format!("Sent clipboard {description} to {count} device(s)")
                    }),
                );
            }
            Err(error) => {
                retry_after = Instant::now() + Duration::from_secs(5);
                let _ = app.emit(
                    "transfer-status",
                    serde_json::json!({
                        "peerId": "clipboard",
                        "state": "error",
                        "message": format!("Clipboard sync failed: {error}; retrying")
                    }),
                );
            }
        }
    }
}

/// Read the current clipboard payload.
pub fn read_clipboard(app: &AppHandle) -> Option<ClipboardPayload> {
    if let Some(files) = read_clipboard_files() {
        return Some(ClipboardPayload::Files(files));
    }
    let clipboard = app.clipboard();
    if let Ok(image) = clipboard.read_image() {
        let rgba = image.rgba().to_vec();
        let size = rgba.len() as u64;
        if !rgba.is_empty() && size <= MAX_CLIPBOARD_IMAGE_BYTES {
            return Some(ClipboardPayload::Image {
                width: image.width(),
                height: image.height(),
                rgba,
            });
        }
    }
    if let Ok(text) = clipboard.read_text() {
        if !text.is_empty() {
            return Some(ClipboardPayload::Text(text));
        }
    }
    None
}

fn read_clipboard_files() -> Option<Vec<ClipboardFile>> {
    #[cfg(target_os = "windows")]
    {
        return read_windows_clipboard_files();
    }
    #[cfg(target_os = "macos")]
    {
        return read_macos_clipboard_files();
    }
    #[allow(unreachable_code)]
    None
}

#[cfg(target_os = "windows")]
fn read_windows_clipboard_files() -> Option<Vec<ClipboardFile>> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows_sys::Win32::UI::Shell::DragQueryFileW;

    const CF_HDROP: u32 = 15;
    let opened = unsafe { OpenClipboard(null_mut()) != 0 };
    if !opened {
        return None;
    }

    let files = unsafe {
        let handle = GetClipboardData(CF_HDROP);
        if handle.is_null() {
            None
        } else {
            let count = DragQueryFileW(handle as _, u32::MAX, null_mut(), 0);
            let mut files = Vec::new();
            for index in 0..count {
                let length = DragQueryFileW(handle as _, index, null_mut(), 0);
                if length == 0 {
                    continue;
                }
                let mut buffer = vec![0u16; length as usize + 1];
                let written = DragQueryFileW(handle as _, index, buffer.as_mut_ptr(), buffer.len() as u32);
                let path = PathBuf::from(String::from_utf16_lossy(&buffer[..written as usize]));
                if path.is_file() {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "clipboard-file".to_string());
                    files.push(ClipboardFile { path, name });
                }
            }
            Some(files)
        }
    };
    unsafe { CloseClipboard(); }
    files.filter(|files| !files.is_empty())
}

#[cfg(target_os = "macos")]
fn read_macos_clipboard_files() -> Option<Vec<ClipboardFile>> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL};
    use objc2_foundation::{NSString, NSURL};

    let value = unsafe {
        NSPasteboard::generalPasteboard()
            .stringForType(NSPasteboardTypeFileURL)
            .map(|value| value.to_string())
    }?;
    let mut files = Vec::new();
    for entry in value.lines().map(str::trim).filter(|entry| !entry.is_empty()) {
        let path = if entry.starts_with("file:") {
            unsafe { NSURL::URLWithString(&NSString::from_str(entry)) }.and_then(|url| url.to_file_path())
        } else {
            Some(PathBuf::from(entry))
        };
        let Some(path) = path else {
            continue;
        };
        if path.is_file() {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "clipboard-file".to_string());
            files.push(ClipboardFile { path, name });
        }
    }
    (!files.is_empty()).then_some(files)
}

/// Apply a payload received from another device when the mode accepts it.
pub fn accept_incoming(app: &AppHandle, payload: &ClipboardPayload) -> Result<(), String> {
    if !current_mode(app).accepts() {
        return Err("clipboard sync is not accepting remote content".to_string());
    }
    write_clipboard(app, payload)?;
    if let Some(sequence) = clipboard_sequence() {
        LAST_SEQUENCE.store(sequence, Ordering::Relaxed);
    }
    Ok(())
}

/// Send the current clipboard on demand (manual mode / "Send now").
pub fn send_now(app: &AppHandle) -> Result<(), String> {
    if let Some(sequence) = clipboard_sequence() {
        LAST_SEQUENCE.store(sequence, Ordering::Relaxed);
    }
    let payload = read_clipboard(app).ok_or_else(|| "clipboard is empty".to_string())?;
    match transfer::send_clipboard_payload(app, &payload) {
        Ok(count) => {
            let _ = app.emit(
                "transfer-status",
                serde_json::json!({
                    "peerId": "clipboard",
                    "state": "sent",
                    "message": format!("Sent clipboard {} to {count} device(s)", describe(&payload))
                }),
            );
            Ok(())
        }
        Err(error) => {
            let _ = app.emit(
                "transfer-status",
                serde_json::json!({
                    "peerId": "clipboard",
                    "state": "error",
                    "message": format!("Clipboard send failed: {error}")
                }),
            );
            Err(error)
        }
    }
}

/// Send a known test value without reading or changing this device's clipboard.
pub fn test_now(app: &AppHandle) -> Result<(), String> {
    let payload = ClipboardPayload::Text("DropAir clipboard sync test".to_string());
    let _ = app.emit(
        "transfer-status",
        serde_json::json!({
            "peerId": "clipboard-test",
            "state": "testing",
            "message": "Sending a clipboard sync test to linked devices"
        }),
    );
    match transfer::send_clipboard_payload(app, &payload) {
        Ok(count) => {
            let _ = app.emit(
                "transfer-status",
                serde_json::json!({
                    "peerId": "clipboard-test",
                    "state": "sent",
                    "message": format!("Clipboard sync test reached {count} device(s)")
                }),
            );
            Ok(())
        }
        Err(error) => {
            let _ = app.emit(
                "transfer-status",
                serde_json::json!({
                    "peerId": "clipboard-test",
                    "state": "error",
                    "message": format!("Clipboard sync test failed: {error}")
                }),
            );
            Err(error)
        }
    }
}

fn write_clipboard(app: &AppHandle, payload: &ClipboardPayload) -> Result<(), String> {
    let clipboard = app.clipboard();
    match payload {
        ClipboardPayload::Text(text) => clipboard
            .write_text(text.clone())
            .map_err(|error| error.to_string()),
        ClipboardPayload::Image {
            width,
            height,
            rgba,
        } => {
            let image = tauri::image::Image::new_owned(rgba.clone(), *width, *height);
            clipboard
                .write_image(&image)
                .map_err(|error| error.to_string())
        }
        ClipboardPayload::Files(_) => Err("file clipboard items are transferred as files".to_string()),
    }
}

fn describe(payload: &ClipboardPayload) -> &'static str {
    match payload {
        ClipboardPayload::Text(_) => "text",
        ClipboardPayload::Image { .. } => "image",
        ClipboardPayload::Files(files) if files.len() == 1 => "file",
        ClipboardPayload::Files(_) => "files",
    }
}

/// Monotonic counter that changes whenever the system clipboard changes.
fn clipboard_sequence() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSPasteboard;
        let pasteboard = NSPasteboard::generalPasteboard();
        Some(pasteboard.changeCount().max(0) as u64)
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
        Some(unsafe { GetClipboardSequenceNumber() } as u64)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}
