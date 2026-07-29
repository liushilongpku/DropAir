use core_foundation::runloop::CFRunLoop;
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CGMouseButton, CallbackResult,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSScreenSaverWindowLevel, NSView, NSWindow, NSWindowCollectionBehavior,
};
use objc2_foundation::{NSRect, NSSize, NSString};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl, WebviewWindowBuilder};

const SHAKE_WINDOW_MS: u128 = 1500;
const MIN_HORIZONTAL_TRAVEL: f64 = 24.0;
const REQUIRED_DIRECTION_CHANGES: u8 = 3;
const TRIGGER_COOLDOWN: Duration = Duration::from_secs(2);
const MONITOR_STARTING: u8 = 0;
const MONITOR_LISTENING: u8 = 1;
const MONITOR_PERMISSION_REQUIRED: u8 = 2;

static MONITOR_STATUS: AtomicU8 = AtomicU8::new(MONITOR_STARTING);
static MOUSE_DOWNS: AtomicU64 = AtomicU64::new(0);
static MOTION_SAMPLES: AtomicU64 = AtomicU64::new(0);
static MAX_DIRECTION_CHANGES: AtomicU8 = AtomicU8::new(0);
static TRIGGERS: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShakeDiagnostics {
    mouse_downs: u64,
    motion_samples: u64,
    max_direction_changes: u8,
    triggers: u64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceButtonState(state_id: CGEventSourceStateID, button: CGMouseButton) -> bool;
}

#[derive(Default)]
struct DragState {
    is_dragging: bool,
    horizontal_extreme_x: Option<f64>,
    direction: i8,
    direction_changes: u8,
    window_started: Option<Instant>,
    last_trigger: Option<Instant>,
}

impl DragState {
    fn reset_motion(&mut self, x: Option<f64>) {
        self.horizontal_extreme_x = x;
        self.direction = 0;
        self.direction_changes = 0;
    }

    fn track_horizontal_motion(&mut self, x: f64) -> bool {
        let Some(extreme_x) = self.horizontal_extreme_x else {
            self.horizontal_extreme_x = Some(x);
            return false;
        };

        match self.direction {
            0 => {
                let travel = x - extreme_x;
                if travel.abs() >= MIN_HORIZONTAL_TRAVEL {
                    self.direction = if travel.is_sign_positive() { 1 } else { -1 };
                    self.horizontal_extreme_x = Some(x);
                }
            }
            1 if x > extreme_x => self.horizontal_extreme_x = Some(x),
            1 if extreme_x - x >= MIN_HORIZONTAL_TRAVEL => {
                self.direction = -1;
                self.direction_changes += 1;
                self.horizontal_extreme_x = Some(x);
            }
            -1 if x < extreme_x => self.horizontal_extreme_x = Some(x),
            -1 if x - extreme_x >= MIN_HORIZONTAL_TRAVEL => {
                self.direction = 1;
                self.direction_changes += 1;
                self.horizontal_extreme_x = Some(x);
            }
            _ => {}
        }

        self.direction_changes >= REQUIRED_DIRECTION_CHANGES
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(
        app,
        "shake-shelf",
        WebviewUrl::App("index.html?shelf=1".into()),
    )
    .title("DropAir Shelf")
    .inner_size(150.0, 130.0)
    .min_inner_size(150.0, 130.0)
    .resizable(true)
    .decorations(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .visible(false)
    .build()?;
    configure_native_window(&window)?;

    let state = Arc::new(Mutex::new(DragState::default()));
    let event_app = app.clone();
    let event_state = Arc::clone(&state);
    thread::spawn(move || watch_drag_events(event_app, event_state));

    let polling_app = app.clone();
    thread::spawn(move || poll_drag_position(polling_app, state));
    Ok(())
}

fn configure_native_window(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let window_ptr = window.ns_window()?;
    let native_window = unsafe { &*(window_ptr.cast::<NSWindow>()) };
    configure_native_window_handle(native_window);
    Ok(())
}

fn configure_native_window_handle(native_window: &NSWindow) {
    let behavior = native_window.collectionBehavior()
        | NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::FullScreenAuxiliary;
    native_window.setCollectionBehavior(behavior);
    native_window.setLevel(NSScreenSaverWindowLevel - 1);
}

fn watch_drag_events(app: AppHandle, state: Arc<Mutex<DragState>>) {
    let callback_state = Arc::clone(&state);
    let callback_app = app.clone();
    let result = CGEventTap::with_enabled(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseDragged,
            CGEventType::LeftMouseUp,
        ],
        move |_proxy, event_type, event| {
            handle_event(&callback_app, &callback_state, event_type, event);
            CallbackResult::Keep
        },
        || {
            set_monitor_status(&app, MONITOR_LISTENING);
            CFRunLoop::run_current();
        },
    );

    if result.is_err() {
        set_monitor_status(&app, MONITOR_PERMISSION_REQUIRED);
    }
}

fn poll_drag_position(app: AppHandle, state: Arc<Mutex<DragState>>) {
    loop {
        if left_button_is_pressed() {
            if let Some((x, y)) = current_pointer_position() {
                process_drag_position(&app, &state, x, y);
            }
        } else if let Ok(mut state) = state.lock() {
            state.is_dragging = false;
        }

        thread::sleep(Duration::from_millis(16));
    }
}

fn left_button_is_pressed() -> bool {
    unsafe {
        CGEventSourceButtonState(
            CGEventSourceStateID::CombinedSessionState,
            CGMouseButton::Left,
        )
    }
}

fn current_pointer_position() -> Option<(f64, f64)> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let event = CGEvent::new(source).ok()?;
    let point = event.location();
    Some((point.x, point.y))
}

fn handle_event(
    app: &AppHandle,
    state_ref: &Arc<Mutex<DragState>>,
    event_type: CGEventType,
    event: &CGEvent,
) {
    let mut state = match state_ref.lock() {
        Ok(state) => state,
        Err(_) => return,
    };

    match event_type {
        CGEventType::LeftMouseDown => {
            MOUSE_DOWNS.fetch_add(1, Ordering::Relaxed);
            state.is_dragging = true;
            state.reset_motion(None);
            state.window_started = Some(Instant::now());
        }
        CGEventType::LeftMouseUp => state.is_dragging = false,
        CGEventType::LeftMouseDragged => {
            let point = event.location();
            drop(state);
            process_drag_position(app, state_ref, point.x, point.y);
        }
        _ => {}
    }
}

fn process_drag_position(app: &AppHandle, state_ref: &Arc<Mutex<DragState>>, x: f64, y: f64) {
    MOTION_SAMPLES.fetch_add(1, Ordering::Relaxed);
    let mut state = match state_ref.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    let now = Instant::now();

    if !state.is_dragging {
        state.is_dragging = true;
        state.reset_motion(Some(x));
        state.window_started = Some(now);
    }

    let window_expired = state
        .window_started
        .is_none_or(|started| now.duration_since(started).as_millis() > SHAKE_WINDOW_MS);
    if window_expired {
        state.reset_motion(Some(x));
        state.window_started = Some(now);
    }

    let shake_detected = state.track_horizontal_motion(x);
    MAX_DIRECTION_CHANGES.fetch_max(state.direction_changes, Ordering::Relaxed);

    if shake_detected
        && state
            .last_trigger
            .is_none_or(|last| now.duration_since(last) >= TRIGGER_COOLDOWN)
    {
        TRIGGERS.fetch_add(1, Ordering::Relaxed);
        state.last_trigger = Some(now);
        state.reset_motion(Some(x));
        state.window_started = Some(now);
        show_shelf(app, x, y);
    }
}

fn set_monitor_status(app: &AppHandle, status: u8) {
    MONITOR_STATUS.store(status, Ordering::Relaxed);
    let _ = app.emit("shake-monitor-status", monitor_status());
}

pub fn monitor_status() -> &'static str {
    match MONITOR_STATUS.load(Ordering::Relaxed) {
        MONITOR_LISTENING => "listening",
        MONITOR_PERMISSION_REQUIRED => "permissionRequired",
        _ => "starting",
    }
}

pub fn diagnostics() -> ShakeDiagnostics {
    ShakeDiagnostics {
        mouse_downs: MOUSE_DOWNS.load(Ordering::Relaxed),
        motion_samples: MOTION_SAMPLES.load(Ordering::Relaxed),
        max_direction_changes: MAX_DIRECTION_CHANGES.load(Ordering::Relaxed),
        triggers: TRIGGERS.load(Ordering::Relaxed),
    }
}

pub fn show_for_test(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("shake-shelf")
        .ok_or_else(|| "shake shelf window is unavailable".to_string())?;
    window.center().map_err(|error| error.to_string())?;
    show_window(&window)
}

pub fn hide(app: &AppHandle) -> Result<(), String> {
    app.get_webview_window("shake-shelf")
        .ok_or_else(|| "shake shelf window is unavailable".to_string())?
        .hide()
        .map_err(|error| error.to_string())
}

pub fn start_dragging(app: &AppHandle) -> Result<(), String> {
    app.get_webview_window("shake-shelf")
        .ok_or_else(|| "shake shelf window is unavailable".to_string())?
        .start_dragging()
        .map_err(|error| error.to_string())
}

pub fn begin_file_drag(app: &AppHandle, path: String) -> Result<(), String> {
    if !Path::new(&path).is_file() {
        return Err("only existing files can be dragged out".to_string());
    }

    let window = app
        .get_webview_window("shake-shelf")
        .ok_or_else(|| "shake shelf window is unavailable".to_string())?;
    let callback_window = window.clone();
    let (sender, receiver) = mpsc::sync_channel(1);

    window
        .run_on_main_thread(move || {
            let result = begin_file_drag_on_main_thread(&callback_window, &path);
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;

    receiver
        .recv()
        .map_err(|_| "native file drag ended unexpectedly".to_string())?
}

fn begin_file_drag_on_main_thread(window: &tauri::WebviewWindow, path: &str) -> Result<(), String> {
    let marker =
        MainThreadMarker::new().ok_or_else(|| "not on the macOS main thread".to_string())?;
    let event = NSApplication::sharedApplication(marker)
        .currentEvent()
        .ok_or_else(|| "no active mouse event was found".to_string())?;
    let view_ptr = window.ns_view().map_err(|error| error.to_string())?;
    let view = unsafe { &*(view_ptr.cast::<NSView>()) };
    let filename = NSString::from_str(path);
    let source_rect = NSRect::new(event.locationInWindow(), NSSize::new(32.0, 32.0));

    #[allow(deprecated)]
    let started = view.dragFile_fromRect_slideBack_event(&filename, source_rect, true, &event);
    if started {
        Ok(())
    } else {
        Err("macOS did not start the file drag".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_shake_from_many_small_trackpad_movements() {
        let mut state = DragState::default();
        let samples = [
            0.0, 6.0, 12.0, 18.0, 25.0, 19.0, 13.0, 7.0, 1.0, 7.0, 13.0, 19.0, 25.0, 19.0, 13.0,
            7.0, 1.0,
        ];

        let detected = samples
            .into_iter()
            .any(|x| state.track_horizontal_motion(x));

        assert!(detected);
        assert_eq!(state.direction_changes, REQUIRED_DIRECTION_CHANGES);
    }

    #[test]
    fn ignores_short_horizontal_jitter() {
        let mut state = DragState::default();
        let detected = [0.0, 8.0, -5.0, 10.0, -7.0, 4.0]
            .into_iter()
            .any(|x| state.track_horizontal_motion(x));

        assert!(!detected);
        assert_eq!(state.direction_changes, 0);
    }
}

fn show_shelf(app: &AppHandle, x: f64, y: f64) {
    if let Some(window) = app.get_webview_window("shake-shelf") {
        let half_width = window
            .inner_size()
            .ok()
            .and_then(|size| {
                window
                    .scale_factor()
                    .ok()
                    .map(|scale| size.to_logical::<f64>(scale))
            })
            .map_or(75.0, |size| size.width / 2.0);
        let _ = window.set_position(LogicalPosition::new(x - half_width, y + 24.0));
        let _ = window.set_visible_on_all_workspaces(true);
        let _ = show_window(&window);
    }
}

fn show_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    window.show().map_err(|error| error.to_string())?;

    let callback_window = window.clone();
    window
        .run_on_main_thread(move || {
            if let Ok(window_ptr) = callback_window.ns_window() {
                let native_window = unsafe { &*(window_ptr.cast::<NSWindow>()) };
                configure_native_window_handle(native_window);
                native_window.orderFrontRegardless();
            }
        })
        .map_err(|error| error.to_string())
}
