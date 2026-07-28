use rdev::{listen, Button, Event, EventType};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, LogicalPosition, Manager, WebviewUrl, WebviewWindowBuilder};

const SHAKE_WINDOW_MS: u128 = 700;
const MIN_HORIZONTAL_TRAVEL: f64 = 24.0;
const REQUIRED_DIRECTION_CHANGES: u8 = 3;
const TRIGGER_COOLDOWN: Duration = Duration::from_secs(2);

#[derive(Default)]
struct DragState {
    is_dragging: bool,
    last_x: Option<f64>,
    direction: i8,
    direction_changes: u8,
    window_started: Option<Instant>,
    last_trigger: Option<Instant>,
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(
        app,
        "shake-shelf",
        WebviewUrl::App("index.html?shelf=1".into()),
    )
    .title("DropAir Shelf")
    .inner_size(360.0, 156.0)
    .min_inner_size(300.0, 130.0)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .build()?;

    let app = app.clone();
    thread::spawn(move || watch_drag_shake(app));
    Ok(())
}

fn watch_drag_shake(app: AppHandle) {
    let state = Arc::new(Mutex::new(DragState::default()));
    let callback_state = Arc::clone(&state);

    // macOS grants this event stream only after the user enables Accessibility access.
    let _ = listen(move |event| handle_event(&app, &callback_state, event));
}

fn handle_event(app: &AppHandle, state: &Arc<Mutex<DragState>>, event: Event) {
    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return,
    };

    match event.event_type {
        EventType::ButtonPress(Button::Left) => {
            state.is_dragging = true;
            state.last_x = None;
            state.direction = 0;
            state.direction_changes = 0;
            state.window_started = Some(Instant::now());
        }
        EventType::ButtonRelease(Button::Left) => state.is_dragging = false,
        EventType::MouseMove { x, y } if state.is_dragging => {
            let now = Instant::now();
            let window_expired = state.window_started.is_none_or(|started| {
                now.duration_since(started).as_millis() > SHAKE_WINDOW_MS
            });
            if window_expired {
                state.direction_changes = 0;
                state.direction = 0;
                state.window_started = Some(now);
            }

            if let Some(last_x) = state.last_x {
                let delta = x - last_x;
                if delta.abs() >= MIN_HORIZONTAL_TRAVEL {
                    let direction = if delta.is_sign_positive() { 1 } else { -1 };
                    if state.direction != 0 && state.direction != direction {
                        state.direction_changes += 1;
                    }
                    state.direction = direction;
                    if state.direction_changes >= REQUIRED_DIRECTION_CHANGES
                        && state
                            .last_trigger
                            .is_none_or(|last| now.duration_since(last) >= TRIGGER_COOLDOWN)
                    {
                        state.last_trigger = Some(now);
                        state.direction_changes = 0;
                        show_shelf(app, x, y);
                    }
                }
            }
            state.last_x = Some(x);
        }
        _ => {}
    }
}

fn show_shelf(app: &AppHandle, x: f64, y: f64) {
    if let Some(window) = app.get_webview_window("shake-shelf") {
        let _ = window.set_position(LogicalPosition::new(x - 180.0, y + 24.0));
        let _ = window.show();
    }
}
