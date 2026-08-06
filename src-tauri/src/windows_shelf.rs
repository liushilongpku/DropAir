use crate::settings::AppSettings;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

const SHELF_LABEL: &str = "shake-shelf";
const SHAKE_WINDOW_MS: u128 = 1500;
const TRIGGER_COOLDOWN: Duration = Duration::from_secs(2);
const MONITOR_STARTING: u8 = 0;
const MONITOR_LISTENING: u8 = 1;

static MONITOR_STATUS: AtomicU8 = AtomicU8::new(MONITOR_STARTING);
static SHAKE_ENABLED: AtomicBool = AtomicBool::new(true);
static SHAKE_SENSITIVITY: AtomicU8 = AtomicU8::new(3);
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

#[derive(Default)]
struct DragState {
    is_dragging: bool,
    horizontal_extreme_x: Option<i32>,
    direction: i8,
    direction_changes: u8,
    window_started: Option<Instant>,
    last_trigger: Option<Instant>,
}

impl DragState {
    fn reset_motion(&mut self, x: Option<i32>) {
        self.horizontal_extreme_x = x;
        self.direction = 0;
        self.direction_changes = 0;
    }

    fn track_horizontal_motion(
        &mut self,
        x: i32,
        min_horizontal_travel: i32,
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
                    self.direction = if travel.is_positive() { 1 } else { -1 };
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

pub fn setup(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    SHAKE_ENABLED.store(settings.shake_enabled, Ordering::Relaxed);
    SHAKE_SENSITIVITY.store(settings.shake_sensitivity.clamp(1, 5), Ordering::Relaxed);
    let _ = get_or_create(app)?;
    let app = app.clone();
    thread::spawn(move || watch_mouse(app));
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

fn watch_mouse(app: AppHandle) {
    set_monitor_status(&app, MONITOR_LISTENING);
    let mut state = DragState::default();
    let mut button_was_down = false;
    loop {
        let button_is_down = left_button_is_pressed();
        if !SHAKE_ENABLED.load(Ordering::Relaxed) {
            state = DragState::default();
            button_was_down = false;
            thread::sleep(Duration::from_millis(50));
            continue;
        }

        if button_is_down {
            if !button_was_down {
                MOUSE_DOWNS.fetch_add(1, Ordering::Relaxed);
            }
            let mut point = POINT { x: 0, y: 0 };
            if unsafe { GetCursorPos(&mut point) } != 0 {
                process_drag_position(&app, &mut state, point.x);
            }
        } else {
            state.is_dragging = false;
            state.reset_motion(None);
        }

        button_was_down = button_is_down;
        thread::sleep(Duration::from_millis(16));
    }
}

fn left_button_is_pressed() -> bool {
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 }
}

fn process_drag_position(app: &AppHandle, state: &mut DragState, x: i32) {
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
        let _ = show_for_test(app);
    }
}

fn shake_thresholds() -> (i32, u8) {
    match SHAKE_SENSITIVITY.load(Ordering::Relaxed).clamp(1, 5) {
        1 => (34, 4),
        2 => (29, 3),
        3 => (24, 3),
        4 => (19, 3),
        _ => (14, 2),
    }
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
