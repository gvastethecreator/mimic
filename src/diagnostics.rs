use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 512 * 1024;
const BACKUP_COUNT: usize = 3;

pub fn log_path() -> PathBuf {
    crate::setup::app_dir_path()
        .join("logs")
        .join("mimic.jsonl")
}

pub fn record_event(level: &str, event: &str) -> io::Result<()> {
    write_event(&log_path(), level, event, MAX_LOG_BYTES)
}

fn write_event(path: &Path, level: &str, event: &str, max_bytes: u64) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= max_bytes)
    {
        rotate(path)?;
    }

    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    let line = serde_json::json!({
        "timestamp_unix_ms": unix_ms,
        "level": sanitize(level),
        "event": sanitize(event),
        "version": env!("CARGO_PKG_VERSION"),
        "process_id": std::process::id(),
    });
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, &line)?;
    file.write_all(b"\n")?;
    file.flush()
}

fn rotate(path: &Path) -> io::Result<()> {
    for index in (1..=BACKUP_COUNT).rev() {
        let destination = backup_path(path, index);
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        let source = if index == 1 {
            path.to_path_buf()
        } else {
            backup_path(path, index - 1)
        };
        if source.exists() {
            fs::rename(source, destination)?;
        }
    }
    Ok(())
}

fn backup_path(path: &Path, index: usize) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mimic.jsonl");
    path.with_file_name(format!("{name}.{index}"))
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, ' ' | '-' | '_' | '.' | ':' | '/')
        })
        .take(96)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_json_lines_and_paths_are_not_accepted_as_free_form_data() {
        let directory = std::env::temp_dir().join(format!(
            "mimic-log-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let path = directory.join("mimic.jsonl");
        write_event(
            &path,
            "warning",
            "Media unavailable C:\\secret\\clip.mp4",
            4096,
        )
        .expect("event should be written");
        let contents = fs::read_to_string(&path).expect("event should be readable");
        let value: serde_json::Value =
            serde_json::from_str(contents.trim()).expect("event must be JSON");
        assert_eq!(value["level"], "warning");
        assert!(!contents.contains('\\'));
        fs::remove_dir_all(directory).expect("test directory should be removable");
    }

    #[test]
    fn oversized_log_rotates_before_append() {
        let directory = std::env::temp_dir().join(format!(
            "mimic-log-rotate-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let path = directory.join("mimic.jsonl");
        fs::create_dir_all(&directory).expect("test directory should exist");
        fs::write(&path, vec![b'x'; 128]).expect("oversized log should exist");
        write_event(&path, "info", "Application started", 64).expect("rotation should pass");
        assert!(backup_path(&path, 1).exists());
        assert!(
            fs::read_to_string(&path)
                .expect("new log should be readable")
                .contains("Application started")
        );
        fs::remove_dir_all(directory).expect("test directory should be removable");
    }
}
