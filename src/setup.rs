use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr;
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

const FFMPEG_URL: &str =
    "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-win32-x64";
const FFMPEG_BYTES: u64 = 82_797_568;
const FFMPEG_SHA256: &str = "04e1307997530f9cf2fe35cba2ca7e8875ca91da02f89d6c7243df819c94ad00";
const UNITY_CAPTURE_URL: &str = "https://raw.githubusercontent.com/schellingb/UnityCapture/3ed54c325e0ad71afcf4f246c07e5e17b3d7f2d2/Install/UnityCaptureFilter64.dll";
const UNITY_CAPTURE_BYTES: u64 = 157_696;
const UNITY_CAPTURE_SHA256: &str =
    "72812f5363d8ecb45632253f8c8c888844b1b62e27616f3c8cc21064ccde25e5";

unsafe extern "system" {
    pub fn ShellExecuteW(
        hwnd: isize,
        lp_operation: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        show_command: i32,
    ) -> isize;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtualCameraBackend {
    Obs,
    UnityCapture,
}

impl VirtualCameraBackend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Obs => "OBS Virtual Camera",
            Self::UnityCapture => "Unity Video Capture",
        }
    }
}

pub fn app_dir_path() -> PathBuf {
    let mut path = dirs_next::data_dir().unwrap_or_else(|| {
        PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| "C:".to_string()))
    });
    path.push("mimic");
    path
}

pub fn get_app_dir() -> PathBuf {
    let path = app_dir_path();
    let _ = std::fs::create_dir_all(&path);
    path
}

pub fn ffmpeg_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from("ffmpeg"), app_dir_path().join("ffmpeg.exe")];
    if let Some(local) = local_executable_sibling("ffmpeg.exe") {
        candidates.push(local);
    }
    candidates
}

pub fn get_ffmpeg_path() -> Option<PathBuf> {
    ffmpeg_candidates()
        .into_iter()
        .find(|candidate| validate_ffmpeg(candidate))
}

pub fn validate_ffmpeg(path: &Path) -> bool {
    Command::new(path)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn get_dll_path() -> PathBuf {
    get_app_dir().join("UnityCaptureFilter64.dll")
}

pub fn available_virtual_camera_backends() -> Vec<VirtualCameraBackend> {
    let mut backends = Vec::new();
    if virtualcam::backend::windows_obs::is_available() {
        backends.push(VirtualCameraBackend::Obs);
    }
    if virtualcam::backend::windows_unity::is_available() {
        backends.push(VirtualCameraBackend::UnityCapture);
    }
    backends
}

pub fn is_unity_capture_registered() -> bool {
    virtualcam::backend::windows_unity::is_available()
}

pub fn download_ffmpeg<F>(progress_callback: F) -> Result<PathBuf, String>
where
    F: Fn(f32),
{
    let destination = get_app_dir().join("ffmpeg.exe");
    download_verified(
        FFMPEG_URL,
        &destination,
        FFMPEG_BYTES,
        FFMPEG_SHA256,
        progress_callback,
    )?;
    if !validate_ffmpeg(&destination) {
        let _ = std::fs::remove_file(&destination);
        return Err("Downloaded FFmpeg did not pass its executable self-check".to_string());
    }
    Ok(destination)
}

pub fn download_driver<F>(progress_callback: F) -> Result<PathBuf, String>
where
    F: Fn(f32),
{
    let destination = get_dll_path();
    download_verified(
        UNITY_CAPTURE_URL,
        &destination,
        UNITY_CAPTURE_BYTES,
        UNITY_CAPTURE_SHA256,
        progress_callback,
    )?;
    Ok(destination)
}

pub fn register_driver_elevated(dll_path: &Path) -> Result<bool, String> {
    if is_unity_capture_registered() {
        return Ok(true);
    }

    let file = wide_string(OsStr::new("regsvr32.exe"));
    let parameters = wide_string(OsStr::new(&format!("/s \"{}\"", dll_path.display())));
    let verb = wide_string(OsStr::new("runas"));

    let result = unsafe {
        ShellExecuteW(
            0,
            verb.as_ptr(),
            file.as_ptr(),
            parameters.as_ptr(),
            ptr::null(),
            5,
        )
    };

    if result as usize <= 32 {
        return Err(format!(
            "Administrator approval was denied or registration could not start (code {})",
            result as usize
        ));
    }

    for _ in 0..120 {
        if is_unity_capture_registered() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(250));
    }

    Err("Registration finished without a detectable Unity Capture device. Restart Mimic or install the driver manually from its official package.".to_string())
}

fn download_verified<F>(
    url: &str,
    destination: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
    progress_callback: F,
) -> Result<(), String>
where
    F: Fn(f32),
{
    if destination.exists()
        && file_matches(destination, expected_bytes, expected_sha256).unwrap_or(false)
    {
        progress_callback(1.0);
        return Ok(());
    }

    let parent = destination
        .parent()
        .ok_or_else(|| "Download destination has no parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create download directory: {error}"))?;
    let partial = append_suffix(destination, ".download");
    let _ = std::fs::remove_file(&partial);

    let result = (|| -> Result<(), String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(90)))
            .build()
            .into();
        let response = agent
            .get(url)
            .call()
            .map_err(|error| format!("Download request failed: {error}"))?;
        let reported_bytes = response
            .headers()
            .get("Content-Length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        if reported_bytes.is_some_and(|bytes| bytes != expected_bytes) {
            return Err(format!(
                "Download size changed upstream (expected {expected_bytes} bytes, server reported {} bytes)",
                reported_bytes.unwrap()
            ));
        }

        let mut reader = response.into_body().into_reader();
        let mut file = File::create(&partial)
            .map_err(|error| format!("Could not create partial download: {error}"))?;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut downloaded = 0_u64;

        loop {
            let bytes_read = reader
                .read(&mut buffer)
                .map_err(|error| format!("Download stream failed: {error}"))?;
            if bytes_read == 0 {
                break;
            }
            file.write_all(&buffer[..bytes_read])
                .map_err(|error| format!("Could not write partial download: {error}"))?;
            hash.update(&buffer[..bytes_read]);
            downloaded += bytes_read as u64;
            progress_callback((downloaded as f32 / expected_bytes as f32).clamp(0.0, 1.0));
        }

        file.sync_all()
            .map_err(|error| format!("Could not flush downloaded file: {error}"))?;
        if downloaded != expected_bytes {
            return Err(format!(
                "Downloaded file is incomplete (expected {expected_bytes} bytes, received {downloaded})"
            ));
        }
        let digest = hash.finalize();
        let actual_sha256 = hex_lower(&digest);
        if actual_sha256 != expected_sha256 {
            return Err(format!(
                "Downloaded file failed integrity verification (expected SHA-256 {expected_sha256}, got {actual_sha256})"
            ));
        }

        replace_file(&partial, destination)
            .map_err(|error| format!("Could not activate downloaded file: {error}"))?;
        progress_callback(1.0);
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

fn file_matches(path: &Path, expected_bytes: u64, expected_sha256: &str) -> io::Result<bool> {
    if std::fs::metadata(path)?.len() != expected_bytes {
        return Ok(false);
    }
    Ok(sha256_file(path)? == expected_sha256)
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hash.update(&buffer[..bytes_read]);
    }
    let digest = hash.finalize();
    Ok(hex_lower(&digest))
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut value, "{byte:02x}").expect("writing to a String cannot fail");
    }
    value
}

fn local_executable_sibling(file_name: &str) -> Option<PathBuf> {
    let mut directory = std::env::current_exe().ok()?;
    directory.pop();
    Some(directory.join(file_name))
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
    fn sha256_matches_known_value() {
        let root = std::env::temp_dir().join(format!("mimic-hash-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("fixture.bin");
        std::fs::write(&path, b"mimic").unwrap();

        assert_eq!(
            sha256_file(&path).unwrap(),
            "692e51978c0e4aa1f130e5cb9a536421f7925c8bae0a2291414791b9f86ee000"
        );
        assert!(
            file_matches(
                &path,
                5,
                "692e51978c0e4aa1f130e5cb9a536421f7925c8bae0a2291414791b9f86ee000"
            )
            .unwrap()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn backend_labels_are_user_facing() {
        assert_eq!(VirtualCameraBackend::Obs.label(), "OBS Virtual Camera");
        assert_eq!(
            VirtualCameraBackend::UnityCapture.label(),
            "Unity Video Capture"
        );
    }
}
