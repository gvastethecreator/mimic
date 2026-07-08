use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;

// Raw Win32 FFI declarations to ensure compiling with no feature issues
#[cfg(target_os = "windows")]
unsafe extern "system" {
    pub fn ShellExecuteW(
        hwnd: isize,
        lpOperation: *const u16,
        lpFile: *const u16,
        lpParameters: *const u16,
        lpDirectory: *const u16,
        nShowCmd: i32,
    ) -> isize;

    pub fn RegOpenKeyExW(
        hKey: isize,
        lpSubKey: *const u16,
        ulOptions: u32,
        samDesired: u32,
        phkResult: *mut isize,
    ) -> i32;

    pub fn RegCloseKey(
        hKey: isize,
    ) -> i32;
}

/// Returns the path to the AppData folder for Mimic
pub fn get_app_dir() -> PathBuf {
    let mut path = dirs_next::data_dir().unwrap_or_else(|| {
        PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| "C:".to_string()))
    });
    path.push("mimic");
    let _ = std::fs::create_dir_all(&path);
    path
}

/// Checks if ffmpeg is in the system PATH
pub fn is_ffmpeg_in_path() -> bool {
    let cmd = if cfg!(target_os = "windows") { "ffmpeg.exe" } else { "ffmpeg" };
    Command::new(cmd)
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Returns the path to ffmpeg.exe if available (system path or local AppData)
pub fn get_ffmpeg_path() -> Option<PathBuf> {
    if is_ffmpeg_in_path() {
        return Some(PathBuf::from("ffmpeg"));
    }
    
    // Check in AppData
    let mut app_ffmpeg = get_app_dir();
    app_ffmpeg.push("ffmpeg.exe");
    if app_ffmpeg.exists() {
        return Some(app_ffmpeg);
    }
    
    // Check in local directory
    if let Ok(mut exe_dir) = std::env::current_exe() {
        exe_dir.pop();
        exe_dir.push("ffmpeg.exe");
        if exe_dir.exists() {
            return Some(exe_dir);
        }
    }
    
    None
}

/// Checks if the Unity Video Capture DLL exists in our AppData directory
pub fn get_dll_path() -> PathBuf {
    let mut dll_path = get_app_dir();
    dll_path.push("UnityCaptureFilter64.dll");
    dll_path
}

/// Downloads a file from a URL using ureq and writes it to a file path.
/// Emits progress between 0.0 and 1.0.
pub fn download_file<F>(url: &str, destination: &Path, progress_cb: F) -> Result<(), String>
where
    F: Fn(f32),
{
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("Failed to call URL: {}", e))?;
    
    let total_size = response
        .header("Content-Length")
        .and_then(|len| len.parse::<u64>().ok())
        .unwrap_or(0);
        
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(destination)
        .map_err(|e| format!("Failed to create destination file: {}", e))?;
        
    let mut buffer = [0u8; 16384];
    let mut downloaded = 0u64;
    
    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|e| format!("Failed reading stream: {}", e))?;
            
        if bytes_read == 0 {
            break;
        }
        
        std::io::Write::write_all(&mut file, &buffer[..bytes_read])
            .map_err(|e| format!("Failed writing file: {}", e))?;
            
        downloaded += bytes_read as u64;
        if total_size > 0 {
            let progress = downloaded as f32 / total_size as f32;
            progress_cb(progress);
        }
    }
    
    Ok(())
}

/// Downloads a lightweight static ffmpeg build for Windows
pub fn download_ffmpeg<F>(progress_cb: F) -> Result<PathBuf, String>
where
    F: Fn(f32),
{
    let destination = get_app_dir().join("ffmpeg.exe");
    // Lightweight portable static ffmpeg URL (e.g. from standard reliable releases or custom proxy)
    // Note: We use a trusted, stable URL for a standalone windows ffmpeg binary.
    // For simplicity, we can fetch a precompiled 64-bit static executable.
    let url = "https://github.com/eugeneware/ffmpeg-static/releases/download/b5.0.1/win32-x64";
    download_file(url, &destination, progress_cb)?;
    Ok(destination)
}

/// Downloads the Unity Capture Filter 64-bit DLL
pub fn download_driver<F>(progress_cb: F) -> Result<PathBuf, String>
where
    F: Fn(f32),
{
    let destination = get_dll_path();
    let url = "https://github.com/schellingb/UnityCapture/raw/master/Install/UnityCaptureFilter64.dll";
    download_file(url, &destination, progress_cb)?;
    Ok(destination)
}

/// Checks if the Unity Video Capture device is registered in Windows DirectShow.
/// We can check if `virtualcam` can initialize it, or inspect registration registry keys.
pub fn is_driver_registered() -> bool {
    // Under Windows, we check if the virtualcam device can be instantiated or if its registry entries are present.
    // Specifically, let's query the CLSID of Unity Capture Filter: {A91FD3C7-15E8-4e89-940E-6F3C01234567}
    #[cfg(target_os = "windows")]
    {
        let subkey = OsStr::new("CLSID\\{A91FD3C7-15E8-4e89-940E-6F3C01234567}")
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>();
            
        let hkey_classes_root = -2147483648isize; // HKEY_CLASSES_ROOT
        let key_read = 0x20019u32;                // KEY_READ
        let mut key = 0isize;
        
        let status = unsafe {
            RegOpenKeyExW(
                hkey_classes_root,
                subkey.as_ptr(),
                0,
                key_read,
                &mut key,
            )
        };
        
        if status == 0 {
            // Key exists, so it's registered!
            // Close the key
            unsafe {
                RegCloseKey(key);
            }
            return true;
        }
    }
    
    false
}

/// Registers the Unity Capture Filter DLL with administrator rights via UAC dialog.
#[cfg(target_os = "windows")]
pub fn register_driver_elevated(dll_path: &Path) -> Result<bool, String> {
    let file = OsStr::new("regsvr32.exe");
    let mut file_wide: Vec<u16> = file.encode_wide().collect();
    file_wide.push(0);

    let args = format!("/s \"{}\"", dll_path.to_string_lossy());
    let args_os = OsStr::new(&args);
    let mut args_wide: Vec<u16> = args_os.encode_wide().collect();
    args_wide.push(0);

    let verb = OsStr::new("runas");
    let mut verb_wide: Vec<u16> = verb.encode_wide().collect();
    verb_wide.push(0);

    let result = unsafe {
        ShellExecuteW(
            0,
            verb_wide.as_ptr(),
            file_wide.as_ptr(),
            args_wide.as_ptr(),
            ptr::null(),
            5, // SW_SHOW
        )
    };
    
    if (result as usize) > 32 {
        Ok(true)
    } else {
        Err(format!("UAC elevation failed or was denied (code: {})", result as usize))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn register_driver_elevated(_dll_path: &Path) -> Result<bool, String> {
    Err("Only supported on Windows".to_string())
}
