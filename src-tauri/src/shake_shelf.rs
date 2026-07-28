use rdev::{listen, Button, Event, EventType};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, LogicalPosition, Manager, WebviewUrl, WebviewWindowBuilder};

const SHAKE_WINDOW_MS: u128 = 900;
const MIN_HORIZONTAL_TRAVEL: f64 = 24.0;
const REQUIRED_DIRECTION_CHANGES: u8 = 3;
const TRIGGER_COOLDOWN: Duration = Duration::from_secs(2);

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
            state.reset_motion(None);
            state.window_started = Some(Instant::now());
        }
        EventType::ButtonRelease(Button::Left) => state.is_dragging = false,
        EventType::MouseMove { x, y } if state.is_dragging => {
            let now = Instant::now();
            let window_expired = state.window_started.is_none_or(|started| {
                now.duration_since(started).as_millis() > SHAKE_WINDOW_MS
            });
            if window_expired {
                state.reset_motion(Some(x));
                state.window_started = Some(now);
            }

            if state.track_horizontal_motion(x)
                && state
                    .last_trigger
                    .is_none_or(|last| now.duration_since(last) >= TRIGGER_COOLDOWN)
            {
                state.last_trigger = Some(now);
                state.reset_motion(Some(x));
                state.window_started = Some(now);
                show_shelf(app, x, y);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_shake_from_many_small_trackpad_movements() {
        let mut state = DragState::default();
        let samples = [
            0.0, 6.0, 12.0, 18.0, 25.0, 19.0, 13.0, 7.0, 1.0, 7.0, 13.0, 19.0, 25.0,
            19.0, 13.0, 7.0, 1.0,
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
        let _ = window.set_position(LogicalPosition::new(x - 180.0, y + 24.0));
        let _ = window.show();
    }
}
