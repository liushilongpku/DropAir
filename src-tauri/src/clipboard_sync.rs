use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

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
        // Remember the change even when we are not sending, so enabling a mode
        // does not replay history.
        LAST_SEQUENCE.store(sequence, Ordering::Relaxed);
        if !mode.auto_sends() || !transfer::has_linked_peers(&app) {
            continue;
        }
        if let Some(payload) = read_clipboard(&app) {
            let description = describe(&payload);
            match transfer::send_clipboard_payload(&app, &payload) {
                Ok(count) => {
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
                    let _ = app.emit(
                        "transfer-status",
                        serde_json::json!({
                            "peerId": "clipboard",
                            "state": "error",
                            "message": format!("Clipboard sync failed: {error}")
                        }),
                    );
                }
            }
        }
    }
}

/// Read the current clipboard payload.
pub fn read_clipboard(app: &AppHandle) -> Option<ClipboardPayload> {
    let clipboard = app.clipboard();
    if let Ok(text) = clipboard.read_text() {
        if !text.is_empty() {
            return Some(ClipboardPayload::Text(text));
        }
    }
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
    None
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
    }
}

fn describe(payload: &ClipboardPayload) -> &'static str {
    match payload {
        ClipboardPayload::Text(_) => "text",
        ClipboardPayload::Image { .. } => "image",
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
