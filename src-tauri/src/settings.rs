use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

const SETTINGS_FILE: &str = "settings.json";
const MIN_SENSITIVITY: u8 = 1;
const MAX_SENSITIVITY: u8 = 5;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    pub shake_enabled: bool,
    pub shake_sensitivity: u8,
    pub shelf_frame: Option<ShelfFrame>,
    pub device_id: String,
    pub device_name: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            shake_enabled: true,
            shake_sensitivity: 3,
            shelf_frame: None,
            device_id: String::new(),
            device_name: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelfFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ShelfFrame {
    pub fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }
}

pub struct SettingsStore {
    path: PathBuf,
    settings: AppSettings,
}

impl SettingsStore {
    pub fn load(app: &tauri::AppHandle) -> Result<Self, String> {
        let path = app
            .path()
            .app_config_dir()
            .map_err(|error| error.to_string())?
            .join(SETTINGS_FILE);
        let mut settings = match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppSettings::default(),
            Err(error) => return Err(error.to_string()),
        };
        settings.shake_sensitivity = clamp_sensitivity(settings.shake_sensitivity);
        settings.shelf_frame = settings.shelf_frame.filter(|frame| frame.is_valid());
        let mut generated_identity = false;
        if settings.device_id.is_empty() {
            settings.device_id = generate_device_id();
            generated_identity = true;
        }
        if settings.device_name.is_empty() {
            settings.device_name = default_device_name();
            generated_identity = true;
        }
        let store = Self { path, settings };
        if generated_identity {
            let _ = store.save();
        }
        Ok(store)
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.clone()
    }

    pub fn set_shake_enabled(&mut self, enabled: bool) -> Result<AppSettings, String> {
        let previous = self.settings.shake_enabled;
        self.settings.shake_enabled = enabled;
        if let Err(error) = self.save() {
            self.settings.shake_enabled = previous;
            return Err(error);
        }
        Ok(self.settings())
    }

    pub fn set_shake_sensitivity(&mut self, sensitivity: u8) -> Result<AppSettings, String> {
        let previous = self.settings.shake_sensitivity;
        self.settings.shake_sensitivity = clamp_sensitivity(sensitivity);
        if let Err(error) = self.save() {
            self.settings.shake_sensitivity = previous;
            return Err(error);
        }
        Ok(self.settings())
    }

    pub fn set_shelf_frame(&mut self, frame: ShelfFrame) -> Result<(), String> {
        if !frame.is_valid() || self.settings.shelf_frame == Some(frame) {
            return Ok(());
        }
        let previous = self.settings.shelf_frame;
        self.settings.shelf_frame = Some(frame);
        if let Err(error) = self.save() {
            self.settings.shelf_frame = previous;
            return Err(error);
        }
        Ok(())
    }

    fn save(&self) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "settings path has no parent directory".to_string())?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary_path = self.path.with_extension("json.tmp");
        let contents =
            serde_json::to_vec_pretty(&self.settings).map_err(|error| error.to_string())?;
        fs::write(&temporary_path, contents).map_err(|error| error.to_string())?;
        fs::rename(&temporary_path, &self.path).map_err(|error| error.to_string())
    }
}

pub fn clamp_sensitivity(sensitivity: u8) -> u8 {
    sensitivity.clamp(MIN_SENSITIVITY, MAX_SENSITIVITY)
}

fn generate_device_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos:x}", std::process::id())
}

fn default_device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "DropAir".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_sensitivity_to_supported_range() {
        assert_eq!(clamp_sensitivity(0), 1);
        assert_eq!(clamp_sensitivity(3), 3);
        assert_eq!(clamp_sensitivity(9), 5);
    }

    #[test]
    fn rejects_invalid_shelf_frames() {
        assert!(!ShelfFrame {
            x: 0.0,
            y: 0.0,
            width: f64::NAN,
            height: 130.0,
        }
        .is_valid());
    }
}
