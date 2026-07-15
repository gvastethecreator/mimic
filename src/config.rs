use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::compositor::PipPosition;

pub const MEDIA_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "gif", "png", "jpg", "jpeg"];

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    pub playlist: Vec<PathBuf>,
    pub selected_webcam: Option<String>,
    pub pip_enabled: bool,
    pub pip_position: PipPosition,
    pub pip_border_radius: u32,
    pub pip_scale: f32,
    pub loop_playlist: bool,
    pub output_resolution_index: usize,
    pub output_fps_index: usize,
    pub current_index: Option<usize>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            playlist: Vec::new(),
            selected_webcam: None,
            pip_enabled: false,
            pip_position: PipPosition::BottomRight,
            pip_border_radius: 16,
            pip_scale: 0.25,
            loop_playlist: true,
            output_resolution_index: 0,
            output_fps_index: 0,
            current_index: None,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct AddMediaReport {
    pub added: usize,
    pub duplicates: usize,
    pub unsupported: usize,
}

#[derive(Debug)]
pub struct ConfigLoad {
    pub config: AppConfig,
    pub warning: Option<String>,
}

impl AppConfig {
    pub fn normalize(&mut self) {
        self.output_resolution_index = self.output_resolution_index.min(2);
        self.output_fps_index = self.output_fps_index.min(1);
        self.pip_scale = if self.pip_scale.is_finite() {
            self.pip_scale.clamp(0.15, 0.45)
        } else {
            Self::default().pip_scale
        };
        self.pip_border_radius = self.pip_border_radius.min(96);

        let mut seen = HashSet::new();
        self.playlist.retain(|path| seen.insert(path_key(path)));
        self.current_index = match (self.current_index, self.playlist.is_empty()) {
            (_, true) => None,
            (Some(index), false) => Some(index.min(self.playlist.len() - 1)),
            (None, false) => None,
        };
    }

    pub fn output_dimensions(&self) -> (u32, u32) {
        match self.output_resolution_index {
            1 => (1920, 1080),
            2 => (640, 480),
            _ => (1280, 720),
        }
    }

    pub fn output_fps(&self) -> f32 {
        if self.output_fps_index == 1 {
            60.0
        } else {
            30.0
        }
    }

    pub fn pip_dimensions(&self) -> (u32, u32) {
        let (output_width, output_height) = self.output_dimensions();
        let width = ((output_width as f32 * self.pip_scale).round() as u32)
            .clamp(160, output_width.saturating_sub(40))
            & !1;
        let height =
            ((width as f32 * 0.75).round() as u32).min(output_height.saturating_sub(40)) & !1;
        (width.max(2), height.max(2))
    }

    pub fn add_media<I>(&mut self, paths: I) -> AddMediaReport
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut report = AddMediaReport::default();
        let mut seen: HashSet<String> = self.playlist.iter().map(|path| path_key(path)).collect();

        for path in paths {
            if !is_supported_media(&path) {
                report.unsupported += 1;
                continue;
            }

            if !seen.insert(path_key(&path)) {
                report.duplicates += 1;
                continue;
            }

            self.playlist.push(path);
            report.added += 1;
        }

        if self.current_index.is_none() && !self.playlist.is_empty() {
            self.current_index = Some(0);
        }
        report
    }

    pub fn remove_media(&mut self, index: usize) -> bool {
        if index >= self.playlist.len() {
            return false;
        }

        self.playlist.remove(index);
        self.current_index = match (self.current_index, self.playlist.is_empty()) {
            (_, true) => None,
            (Some(current), false) if current > index => Some(current - 1),
            (Some(current), false) if current == index => Some(index.min(self.playlist.len() - 1)),
            (current, false) => current,
        };
        true
    }

    pub fn advance_after_end(&mut self) -> bool {
        let Some(current) = self.current_index else {
            return false;
        };
        if current + 1 < self.playlist.len() {
            self.current_index = Some(current + 1);
            true
        } else if self.loop_playlist && !self.playlist.is_empty() {
            self.current_index = Some(0);
            true
        } else {
            false
        }
    }
}

pub fn load(path: &Path) -> ConfigLoad {
    if !path.exists() {
        return ConfigLoad {
            config: AppConfig::default(),
            warning: None,
        };
    }

    match std::fs::read_to_string(path) {
        Ok(json) => match serde_json::from_str::<AppConfig>(&json) {
            Ok(mut config) => {
                config.normalize();
                ConfigLoad {
                    config,
                    warning: None,
                }
            }
            Err(error) => ConfigLoad {
                config: AppConfig::default(),
                warning: Some(format!(
                    "Settings could not be read and safe defaults were loaded: {error}"
                )),
            },
        },
        Err(error) => ConfigLoad {
            config: AppConfig::default(),
            warning: Some(format!(
                "Settings could not be opened and safe defaults were loaded: {error}"
            )),
        },
    }
}

pub fn save(path: &Path, config: &AppConfig) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Settings path has no parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create settings directory: {error}"))?;

    let temporary = append_suffix(path, ".tmp");
    let result = (|| -> Result<(), String> {
        let mut file = File::create(&temporary)
            .map_err(|error| format!("Could not create temporary settings file: {error}"))?;
        serde_json::to_writer_pretty(&mut file, config)
            .map_err(|error| format!("Could not serialize settings: {error}"))?;
        file.write_all(b"\n")
            .map_err(|error| format!("Could not finish settings file: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Could not flush settings to disk: {error}"))?;
        replace_file(&temporary, path)
            .map_err(|error| format!("Could not replace settings file: {error}"))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub fn is_supported_media(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            MEDIA_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    let source = wide_string(source.as_os_str());
    let destination = wide_string(destination.as_os_str());
    let result = unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                | windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wide_string(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_clamps_values_and_preserves_missing_playlist_entries() {
        let missing = PathBuf::from(r"Z:\offline\clip.mp4");
        let mut config = AppConfig {
            playlist: vec![missing.clone(), missing.clone()],
            pip_scale: f32::NAN,
            pip_border_radius: 400,
            output_resolution_index: 99,
            output_fps_index: 99,
            current_index: Some(10),
            ..AppConfig::default()
        };

        config.normalize();

        assert_eq!(config.playlist, vec![missing]);
        assert_eq!(config.pip_scale, AppConfig::default().pip_scale);
        assert_eq!(config.pip_border_radius, 96);
        assert_eq!(config.output_resolution_index, 2);
        assert_eq!(config.output_fps_index, 1);
        assert_eq!(config.current_index, Some(0));
    }

    #[test]
    fn playlist_add_remove_and_advance_are_consistent() {
        let mut config = AppConfig::default();
        let report = config.add_media([
            PathBuf::from(r"C:\clips\one.MP4"),
            PathBuf::from(r"c:/clips/one.mp4"),
            PathBuf::from(r"C:\clips\notes.txt"),
            PathBuf::from(r"C:\clips\two.mov"),
        ]);

        assert_eq!(
            report,
            AddMediaReport {
                added: 2,
                duplicates: 1,
                unsupported: 1,
            }
        );
        assert_eq!(config.current_index, Some(0));
        assert!(config.advance_after_end());
        assert_eq!(config.current_index, Some(1));
        assert!(config.advance_after_end());
        assert_eq!(config.current_index, Some(0));
        assert!(config.remove_media(0));
        assert_eq!(config.current_index, Some(0));
        assert_eq!(config.playlist.len(), 1);
    }

    #[test]
    fn pip_scale_changes_capture_dimensions_with_even_bounds() {
        let mut config = AppConfig::default();
        assert_eq!(config.pip_dimensions(), (320, 240));

        config.output_resolution_index = 1;
        config.pip_scale = 0.4;
        let (width, height) = config.pip_dimensions();

        assert_eq!((width, height), (768, 576));
        assert_eq!(width % 2, 0);
        assert_eq!(height % 2, 0);
    }

    #[test]
    fn save_and_load_round_trip() {
        let root = std::env::temp_dir().join(format!(
            "mimic-config-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("config.json");
        let mut expected = AppConfig {
            pip_enabled: true,
            ..AppConfig::default()
        };
        expected.playlist.push(PathBuf::from(r"C:\clips\one.mp4"));
        expected.current_index = Some(0);

        save(&path, &expected).unwrap();
        let loaded = load(&path);

        assert_eq!(loaded.warning, None);
        assert_eq!(loaded.config, expected);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_config_returns_defaults_with_actionable_warning() {
        let root = std::env::temp_dir().join(format!("mimic-corrupt-test-{}", std::process::id()));
        let path = root.join("config.json");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, "{ definitely not json").unwrap();

        let loaded = load(&path);

        assert_eq!(loaded.config, AppConfig::default());
        assert!(loaded.warning.unwrap().contains("safe defaults"));
        let _ = std::fs::remove_dir_all(root);
    }
}
