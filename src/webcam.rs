use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub struct WebcamCapture {
    child: Option<Child>,
    frame_rx: Receiver<Vec<u8>>,
    stop_tx: Sender<()>,
    width: u32,
    height: u32,
}

impl WebcamCapture {
    pub fn new(
        ffmpeg_path: &Path,
        device_name: &str,
        width: u32,
        height: u32,
        fps: f32,
    ) -> Result<Self, String> {
        let (frame_tx, frame_rx) = channel::<Vec<u8>>();
        let (stop_tx, stop_rx) = channel::<()>();
        
        let ffmpeg_path_buf = ffmpeg_path.to_path_buf();
        let device_name_string = device_name.to_string();
        
        let frame_size = (width * height * 3) as usize;
        
        let mut child = Command::new(&ffmpeg_path_buf)
            .arg("-f")
            .arg("dshow")
            .arg("-i")
            .arg(format!("video={}", device_name_string))
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgb24")
            .arg("-s")
            .arg(format!("{}x{}", width, height))
            .arg("-r")
            .arg(format!("{}", fps))
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn webcam FFmpeg: {}", e))?;
            
        let mut stdout = child.stdout.take().ok_or("Failed to open webcam stdout pipe")?;
        
        thread::spawn(move || {
            let mut buffer = vec![0u8; frame_size];
            
            loop {
                // Check if stopped
                if stop_rx.try_recv().is_ok() {
                    let _ = child.kill();
                    break;
                }
                
                use std::io::Read;
                match stdout.read_exact(&mut buffer) {
                    Ok(_) => {
                        let _ = frame_tx.send(buffer.clone());
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        // Webcam stream stopped
                        let _ = child.kill();
                        break;
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        });
        
        Ok(Self {
            child: None, // Handle killed inside background thread
            frame_rx,
            stop_tx,
            width,
            height,
        })
    }
    
    pub fn next_frame(&self) -> Option<Vec<u8>> {
        self.frame_rx.try_recv().ok()
    }
    
    pub fn stop(self) {
        let _ = self.stop_tx.send(());
    }
}

/// Lists all physical DirectShow video devices by querying FFmpeg.
pub fn list_webcams(ffmpeg_path: &Path) -> Vec<String> {
    let mut webcams = Vec::new();
    
    let output = Command::new(ffmpeg_path)
        .arg("-f")
        .arg("dshow")
        .arg("-list_devices")
        .arg("true")
        .arg("-i")
        .arg("dummy")
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .output();
        
    if let Ok(out) = output {
        let stderr_str = String::from_utf8_lossy(&out.stderr);
        let mut parsing_video = false;
        
        for line in stderr_str.lines() {
            // Check if we are starting the video devices list
            if line.contains("DirectShow video devices") {
                parsing_video = true;
                continue;
            }
            // Check if we hit the audio devices list, and stop parsing
            if line.contains("DirectShow audio devices") {
                parsing_video = false;
                continue;
            }
            
            if parsing_video {
                // A device line looks like:
                // [dshow @ ...]  "Integrated Camera"
                if let Some(quote_start) = line.find('"') {
                    if let Some(quote_end) = line[quote_start + 1..].find('"') {
                        let device_name = &line[quote_start + 1..quote_start + 1 + quote_end];
                        // Avoid listing generic names or double entries
                        if !device_name.is_empty() && !webcams.contains(&device_name.to_string()) {
                            webcams.push(device_name.to_string());
                        }
                    }
                }
            }
        }
    }
    
    webcams
}
