use crate::settings::{AppSettings, SettingsStore, ShelfFrame};
use core_foundation::runloop::CFRunLoop;
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CGMouseButton, CallbackResult,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::{rc::Retained, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSEvent, NSEventType, NSPanel, NSPasteboard,
    NSPasteboardNameDrag, NSPasteboardTypeFileURL, NSPasteboardTypeString, NSStatusWindowLevel,
    NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const SHAKE_WINDOW_MS: u128 = 1500;
const TRIGGER_COOLDOWN: Duration = Duration::from_secs(2);
const FRAME_SAVE_DELAY: Duration = Duration::from_millis(500);
const MIN_SHELF_WIDTH: f64 = 150.0;
const MIN_SHELF_HEIGHT: f64 = 130.0;
const MONITOR_STARTING: u8 = 0;
const MONITOR_LISTENING: u8 = 1;
const MONITOR_PERMISSION_REQUIRED: u8 = 2;

static MONITOR_STATUS: AtomicU8 = AtomicU8::new(MONITOR_STARTING);
static MOUSE_DOWNS: AtomicU64 = AtomicU64::new(0);
static MOTION_SAMPLES: AtomicU64 = AtomicU64::new(0);
static MAX_DIRECTION_CHANGES: AtomicU8 = AtomicU8::new(0);
static TRIGGERS: AtomicU64 = AtomicU64::new(0);
static SHELF_PANEL: AtomicUsize = AtomicUsize::new(0);
static SHELF_VISIBLE: AtomicBool = AtomicBool::new(false);
static SHAKE_ENABLED: AtomicBool = AtomicBool::new(true);
static SHAKE_SENSITIVITY: AtomicU8 = AtomicU8::new(3);
static SHELF_FRAME_TRACKER: OnceLock<Mutex<FrameTracker>> = OnceLock::new();

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
    observed_drag_event: bool,
    horizontal_extreme_x: Option<f64>,
    direction: i8,
    direction_changes: u8,
    window_started: Option<Instant>,
    last_trigger: Option<Instant>,
}

struct FrameTracker {
    observed: Option<ShelfFrame>,
    observed_since: Instant,
    persisted: Option<ShelfFrame>,
}

#[derive(Clone, Copy)]
enum PanelPlacement {
    Keep,
    Pointer,
}

impl DragState {
    fn reset_motion(&mut self, x: Option<f64>) {
        self.horizontal_extreme_x = x;
        self.direction = 0;
        self.direction_changes = 0;
    }

    fn track_horizontal_motion(
        &mut self,
        x: f64,
        min_horizontal_travel: f64,
        required_direction_changes: u8,
    ) -> bool {
        let Some(extreme_x) = self.horizontal_extreme_x else {
            self.horizontal_extreme_x = Some(x);
            return false;
        };

        match self.direction {
            0 => {
                let travel = x - extreme_x;
                if travel.abs() >= min_horizontal_travel {
                    self.direction = if travel.is_sign_positive() { 1 } else { -1 };
                    self.horizontal_extreme_x = Some(x);
                }
            }
            1 if x > extreme_x => self.horizontal_extreme_x = Some(x),
            1 if extreme_x - x >= min_horizontal_travel => {
                self.direction = -1;
                self.direction_changes += 1;
                self.horizontal_extreme_x = Some(x);
            }
            -1 if x < extreme_x => self.horizontal_extreme_x = Some(x),
            -1 if x - extreme_x >= min_horizontal_travel => {
                self.direction = 1;
                self.direction_changes += 1;
                self.horizontal_extreme_x = Some(x);
            }
            _ => {}
        }

        self.direction_changes >= required_direction_changes
    }
}

pub fn setup(app: &AppHandle, settings: &AppSettings) -> tauri::Result<()> {
    SHAKE_ENABLED.store(settings.shake_enabled, Ordering::Relaxed);
    SHAKE_SENSITIVITY.store(settings.shake_sensitivity, Ordering::Relaxed);
    let _ = SHELF_FRAME_TRACKER.set(Mutex::new(FrameTracker {
        observed: settings.shelf_frame,
        observed_since: Instant::now(),
        persisted: settings.shelf_frame,
    }));
    let window = WebviewWindowBuilder::new(
        app,
        "shake-shelf",
        WebviewUrl::App("index.html?shelf=1".into()),
    )
    .title("DropAir Shelf")
    .inner_size(MIN_SHELF_WIDTH, MIN_SHELF_HEIGHT)
    .min_inner_size(MIN_SHELF_WIDTH, MIN_SHELF_HEIGHT)
    .resizable(true)
    .decorations(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .visible(false)
    .build()?;
    create_shelf_panel(&window, settings.shelf_frame)?;

    let state = Arc::new(Mutex::new(DragState::default()));
    let event_app = app.clone();
    let event_state = Arc::clone(&state);
    thread::spawn(move || watch_drag_events(event_app, event_state));

    let polling_app = app.clone();
    thread::spawn(move || poll_drag_position(polling_app, state));

    let shelf_app = app.clone();
    thread::spawn(move || keep_shelf_front(shelf_app));
    Ok(())
}

fn create_shelf_panel(
    window: &tauri::WebviewWindow,
    restored_frame: Option<ShelfFrame>,
) -> tauri::Result<()> {
    let marker = MainThreadMarker::new().expect("shelf setup must run on the macOS main thread");
    let source_ptr = window.ns_window()?;
    let source_window = unsafe { &*(source_ptr.cast::<NSWindow>()) };
    let content_view = source_window
        .contentView()
        .expect("Tauri shelf window must have a content view");
    source_window.setContentView(None);

    let content_rect = restored_frame
        .filter(|frame| frame.is_valid())
        .map(|frame| {
            NSRect::new(
                NSPoint::new(frame.x, frame.y),
                NSSize::new(
                    frame.width.max(MIN_SHELF_WIDTH),
                    frame.height.max(MIN_SHELF_HEIGHT),
                ),
            )
        })
        .unwrap_or_else(|| {
            NSRect::new(
                source_window.frame().origin,
                NSSize::new(MIN_SHELF_WIDTH, MIN_SHELF_HEIGHT),
            )
        });
    let style = NSWindowStyleMask::Resizable | NSWindowStyleMask::NonactivatingPanel;
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(marker),
        content_rect,
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    panel.setContentView(Some(&content_view));
    panel.setContentMinSize(NSSize::new(MIN_SHELF_WIDTH, MIN_SHELF_HEIGHT));
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(true);
    unsafe { panel.setReleasedWhenClosed(false) };
    configure_native_window_handle(&panel);

    let panel_ptr = Retained::into_raw(panel) as usize;
    SHELF_PANEL.store(panel_ptr, Ordering::Release);
    Ok(())
}

fn shelf_panel() -> Result<&'static NSPanel, String> {
    MainThreadMarker::new().ok_or_else(|| "not on the macOS main thread".to_string())?;
    let panel_ptr = SHELF_PANEL.load(Ordering::Acquire);
    if panel_ptr == 0 {
        return Err("shake shelf panel is unavailable".to_string());
    }
    Ok(unsafe { &*(panel_ptr as *const NSPanel) })
}

fn configure_native_window_handle(native_window: &NSWindow) {
    let mut behavior = native_window.collectionBehavior();
    behavior &= !NSWindowCollectionBehavior::MoveToActiveSpace;
    behavior &= !NSWindowCollectionBehavior::Managed;
    behavior &= !NSWindowCollectionBehavior::Transient;
    behavior &= !NSWindowCollectionBehavior::FullScreenPrimary;
    behavior &= !NSWindowCollectionBehavior::FullScreenNone;
    behavior |= NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::Stationary
        | NSWindowCollectionBehavior::IgnoresCycle
        | NSWindowCollectionBehavior::FullScreenAuxiliary;
    native_window.setCollectionBehavior(behavior);
    native_window.setCanHide(false);
    native_window.setHidesOnDeactivate(false);
    native_window.setLevel(NSStatusWindowLevel);
}

fn keep_shelf_front(app: AppHandle) {
    loop {
        if SHELF_VISIBLE.load(Ordering::Acquire) {
            if let Some(window) = app.get_webview_window("shake-shelf") {
                let _ = bring_to_front(&window, PanelPlacement::Keep);
                let frame_app = app.clone();
                let _ = window.run_on_main_thread(move || {
                    if let Ok(panel) = shelf_panel() {
                        observe_shelf_frame(&frame_app, panel.frame());
                    }
                });
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
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
        CGEventType::LeftMouseDown => state.observed_drag_event = false,
        CGEventType::LeftMouseDragged => state.observed_drag_event = true,
        _ => {}
    }

    if matches!(event_type, CGEventType::LeftMouseUp) {
        let observed_drag_event = state.observed_drag_event;
        state.is_dragging = false;
        state.observed_drag_event = false;
        drop(state);
        if observed_drag_event {
            capture_text_drop_if_over_app(app);
        }
        return;
    }

    if !SHAKE_ENABLED.load(Ordering::Relaxed) {
        state.is_dragging = false;
        state.reset_motion(None);
        state.window_started = None;
        return;
    }

    match event_type {
        CGEventType::LeftMouseDown => {
            MOUSE_DOWNS.fetch_add(1, Ordering::Relaxed);
            state.is_dragging = true;
            state.reset_motion(None);
            state.window_started = Some(Instant::now());
        }
        CGEventType::LeftMouseDragged => {
            let point = event.location();
            drop(state);
            process_drag_position(app, state_ref, point.x, point.y);
        }
        _ => {}
    }
}

fn process_drag_position(app: &AppHandle, state_ref: &Arc<Mutex<DragState>>, x: f64, y: f64) {
    let mut state = match state_ref.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    if !SHAKE_ENABLED.load(Ordering::Relaxed) {
        state.is_dragging = false;
        state.reset_motion(None);
        state.window_started = None;
        return;
    }
    MOTION_SAMPLES.fetch_add(1, Ordering::Relaxed);
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

    let (min_horizontal_travel, required_direction_changes) = shake_thresholds();
    let shake_detected =
        state.track_horizontal_motion(x, min_horizontal_travel, required_direction_changes);
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
        if let Some(text) = dragged_plain_text() {
            let _ = crate::add_shelf_text_to_app(app, text);
        }
        show_shelf(app, x, y);
    }
}

fn dragged_plain_text() -> Option<String> {
    // AppKit owns these immutable pasteboard name and type constants for the process lifetime.
    let text = unsafe {
        let pasteboard = NSPasteboard::pasteboardWithName(NSPasteboardNameDrag);
        if pasteboard.stringForType(NSPasteboardTypeFileURL).is_some() {
            return None;
        }
        pasteboard
            .stringForType(NSPasteboardTypeString)?
            .to_string()
    };
    (!text.trim().is_empty()).then_some(text)
}

fn capture_text_drop_if_over_app(app: &AppHandle) {
    let Some(text) = dragged_plain_text() else {
        return;
    };
    let Some(main_window) = app.get_webview_window("main") else {
        return;
    };
    let dispatch_window = main_window.clone();
    let drop_app = app.clone();
    let _ = dispatch_window.run_on_main_thread(move || {
        let pointer = NSEvent::mouseLocation();
        let inside_shelf = SHELF_VISIBLE.load(Ordering::Acquire)
            && shelf_panel()
                .map(|panel| point_is_inside(pointer, panel.frame()))
                .unwrap_or(false);
        let inside_main = main_window
            .ns_window()
            .ok()
            .map(|pointer| unsafe { &*(pointer.cast::<NSWindow>()) })
            .is_some_and(|window| window.isVisible() && point_is_inside(pointer, window.frame()));
        if inside_shelf || inside_main {
            let _ = crate::add_shelf_text_to_app(&drop_app, text);
        }
    });
}

fn point_is_inside(point: NSPoint, frame: NSRect) -> bool {
    point.x >= frame.origin.x
        && point.x <= frame.origin.x + frame.size.width
        && point.y >= frame.origin.y
        && point.y <= frame.origin.y + frame.size.height
}

fn set_monitor_status(app: &AppHandle, status: u8) {
    MONITOR_STATUS.store(status, Ordering::Relaxed);
    let _ = app.emit("shake-monitor-status", monitor_status());
}

pub fn monitor_status() -> &'static str {
    if !SHAKE_ENABLED.load(Ordering::Relaxed) {
        return "disabled";
    }
    match MONITOR_STATUS.load(Ordering::Relaxed) {
        MONITOR_LISTENING => "listening",
        MONITOR_PERMISSION_REQUIRED => "permissionRequired",
        _ => "starting",
    }
}

pub fn set_enabled(app: &AppHandle, enabled: bool) {
    SHAKE_ENABLED.store(enabled, Ordering::Relaxed);
    let _ = app.emit("shake-monitor-status", monitor_status());
}

pub fn set_sensitivity(sensitivity: u8) {
    SHAKE_SENSITIVITY.store(sensitivity.clamp(1, 5), Ordering::Relaxed);
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
    show_window(&window, PanelPlacement::Keep)
}

pub fn hide(app: &AppHandle) -> Result<(), String> {
    SHELF_VISIBLE.store(false, Ordering::Release);
    let window = app
        .get_webview_window("shake-shelf")
        .ok_or_else(|| "shake shelf window is unavailable".to_string())?;
    let frame_app = app.clone();
    window
        .run_on_main_thread(move || {
            if let Ok(panel) = shelf_panel() {
                persist_shelf_frame(&frame_app, panel.frame());
                panel.orderOut(None);
            }
        })
        .map_err(|error| error.to_string())
}

pub fn start_dragging(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("shake-shelf")
        .ok_or_else(|| "shake shelf window is unavailable".to_string())?;
    let frame_app = app.clone();
    window
        .run_on_main_thread(move || {
            let Some(marker) = MainThreadMarker::new() else {
                return;
            };
            let Some(event) = NSApplication::sharedApplication(marker).currentEvent() else {
                return;
            };
            if let Ok(panel) = shelf_panel() {
                panel.performWindowDragWithEvent(&event);
                persist_shelf_frame(&frame_app, panel.frame());
            }
        })
        .map_err(|error| error.to_string())
}

pub fn begin_file_drag(app: &AppHandle, path: String) -> Result<(), String> {
    if !Path::new(&path).is_file() {
        return Err("only existing files can be dragged out".to_string());
    }

    if MainThreadMarker::new().is_some() {
        return begin_file_drag_on_main_thread(&path);
    }

    let window = app
        .get_webview_window("shake-shelf")
        .ok_or_else(|| "shake shelf window is unavailable".to_string())?;
    window
        .run_on_main_thread(move || {
            let _ = begin_file_drag_on_main_thread(&path);
        })
        .map_err(|error| error.to_string())
}

fn begin_file_drag_on_main_thread(path: &str) -> Result<(), String> {
    let marker =
        MainThreadMarker::new().ok_or_else(|| "not on the macOS main thread".to_string())?;
    let event = NSApplication::sharedApplication(marker)
        .currentEvent()
        .ok_or_else(|| "no active mouse event was found".to_string())?;
    if !matches!(
        event.r#type(),
        NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged
    ) {
        return Err("the active event cannot start a file drag".to_string());
    }
    let view = shelf_panel()?
        .contentView()
        .ok_or_else(|| "shake shelf panel has no content view".to_string())?;
    let filename = NSString::from_str(path);
    let source_point = view.convertPoint_fromView(event.locationInWindow(), None);
    let source_rect = NSRect::new(source_point, NSSize::new(32.0, 32.0));

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
            .any(|x| state.track_horizontal_motion(x, 24.0, 3));

        assert!(detected);
        assert_eq!(state.direction_changes, 3);
    }

    #[test]
    fn ignores_short_horizontal_jitter() {
        let mut state = DragState::default();
        let detected = [0.0, 8.0, -5.0, 10.0, -7.0, 4.0]
            .into_iter()
            .any(|x| state.track_horizontal_motion(x, 24.0, 3));

        assert!(!detected);
        assert_eq!(state.direction_changes, 0);
    }

    #[test]
    fn higher_sensitivity_reduces_required_motion() {
        assert_eq!(thresholds_for_sensitivity(1), (34.0, 4));
        assert_eq!(thresholds_for_sensitivity(3), (24.0, 3));
        assert_eq!(thresholds_for_sensitivity(5), (14.0, 2));
    }
}

fn shake_thresholds() -> (f64, u8) {
    thresholds_for_sensitivity(SHAKE_SENSITIVITY.load(Ordering::Relaxed))
}

fn thresholds_for_sensitivity(sensitivity: u8) -> (f64, u8) {
    match sensitivity.clamp(1, 5) {
        1 => (34.0, 4),
        2 => (29.0, 3),
        3 => (24.0, 3),
        4 => (19.0, 3),
        _ => (14.0, 2),
    }
}

fn shelf_frame(rect: NSRect) -> ShelfFrame {
    fn round_half(value: f64) -> f64 {
        (value * 2.0).round() / 2.0
    }

    ShelfFrame {
        x: round_half(rect.origin.x),
        y: round_half(rect.origin.y),
        width: round_half(rect.size.width.max(MIN_SHELF_WIDTH)),
        height: round_half(rect.size.height.max(MIN_SHELF_HEIGHT)),
    }
}

fn observe_shelf_frame(app: &AppHandle, rect: NSRect) {
    let frame = shelf_frame(rect);
    let Some(tracker) = SHELF_FRAME_TRACKER.get() else {
        return;
    };
    let should_persist = {
        let Ok(mut tracker) = tracker.lock() else {
            return;
        };
        if tracker.observed != Some(frame) {
            tracker.observed = Some(frame);
            tracker.observed_since = Instant::now();
            false
        } else {
            tracker.persisted != Some(frame) && tracker.observed_since.elapsed() >= FRAME_SAVE_DELAY
        }
    };
    if should_persist {
        persist_shelf_frame(app, rect);
    }
}

fn persist_shelf_frame(app: &AppHandle, rect: NSRect) {
    let frame = shelf_frame(rect);
    let state = app.state::<Mutex<SettingsStore>>();
    let saved = state
        .lock()
        .map_err(|_| "failed to lock settings".to_string())
        .and_then(|mut store| store.set_shelf_frame(frame))
        .is_ok();
    if saved {
        if let Some(tracker) = SHELF_FRAME_TRACKER.get() {
            if let Ok(mut tracker) = tracker.lock() {
                tracker.observed = Some(frame);
                tracker.observed_since = Instant::now();
                tracker.persisted = Some(frame);
            }
        }
    }
}

fn show_shelf(app: &AppHandle, _x: f64, _y: f64) {
    if let Some(window) = app.get_webview_window("shake-shelf") {
        let _ = show_window(&window, PanelPlacement::Pointer);
    }
}

fn show_window(window: &tauri::WebviewWindow, placement: PanelPlacement) -> Result<(), String> {
    SHELF_VISIBLE.store(true, Ordering::Release);
    bring_to_front(window, placement)
}

fn bring_to_front(window: &tauri::WebviewWindow, placement: PanelPlacement) -> Result<(), String> {
    window
        .run_on_main_thread(move || {
            if let Ok(panel) = shelf_panel() {
                match placement {
                    PanelPlacement::Keep => {}
                    PanelPlacement::Pointer => {
                        let pointer = NSEvent::mouseLocation();
                        let panel_width = panel.frame().size.width;
                        panel.setFrameTopLeftPoint(NSPoint::new(
                            pointer.x - panel_width / 2.0,
                            pointer.y - 24.0,
                        ));
                    }
                }
                panel.orderFrontRegardless();
            }
        })
        .map_err(|error| error.to_string())
}
