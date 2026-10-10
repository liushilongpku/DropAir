use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
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
    pub download_directory: Option<String>,
    pub clipboard_mode: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            shake_enabled: true,
            shake_sensitivity: 3,
            shelf_frame: None,
            device_id: String::new(),
            device_name: String::new(),
            download_directory: None,
            clipboard_mode: "off".to_string(),
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
        if settings.device_name.trim().is_empty() || settings.device_name.trim() == "DropAir" {
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

    pub fn set_download_directory(&mut self, directory: Option<String>) -> Result<AppSettings, String> {
        let previous = self.settings.download_directory.clone();
        self.settings.download_directory = directory;
        if let Err(error) = self.save() {
            self.settings.download_directory = previous;
            return Err(error);
        }
        Ok(self.settings())
    }

    pub fn set_device_name(&mut self, name: String) -> Result<AppSettings, String> {
        let name = sanitize_device_name(&name);
        if name.is_empty() {
            return Err("device name cannot be empty".to_string());
        }
        let previous = self.settings.device_name.clone();
        self.settings.device_name = name;
        if let Err(error) = self.save() {
            self.settings.device_name = previous;
            return Err(error);
        }
        Ok(self.settings())
    }

    pub fn set_clipboard_mode(&mut self, mode: String) -> Result<AppSettings, String> {
        let previous = self.settings.clipboard_mode.clone();
        self.settings.clipboard_mode = normalize_clipboard_mode(&mode).to_string();
        if let Err(error) = self.save() {
            self.settings.clipboard_mode = previous;
            return Err(error);
        }
        Ok(self.settings())
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

pub fn normalize_clipboard_mode(mode: &str) -> &'static str {
    match mode {
        "manual" => "manual",
        "push" => "push",
        "sync" => "sync",
        _ => "off",
    }
}

fn generate_device_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos:x}", std::process::id())
}

fn default_device_name() -> String {
    let mut candidates = Vec::new();
    #[cfg(target_os = "macos")]
    candidates.push(command_output("scutil", &["--get", "ComputerName"]));
    #[cfg(target_os = "windows")]
    candidates.push(command_output("hostname", &[]));
    candidates.extend([
        std::env::var("COMPUTERNAME").ok(),
        std::env::var("HOSTNAME").ok(),
        command_output("hostname", &[]),
    ]);
    candidates
        .into_iter()
        .flatten()
        .map(|name| sanitize_device_name(&name))
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| "DropAir".to_string())
}

fn command_output(command: &str, args: &[&str]) -> Option<String> {
    Command::new(command)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

pub fn sanitize_device_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_control() || character == '|' {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
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

    #[test]
    fn sanitizes_device_names_for_discovery() {
        assert_eq!(sanitize_device_name(" Mac | Office\n"), "Mac Office");
    }
}
