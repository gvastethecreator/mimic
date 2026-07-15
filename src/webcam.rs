use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Default)]
pub struct WebcamUpdate {
    pub latest_frame: Option<Vec<u8>>,
    pub error: Option<String>,
}

pub struct WebcamCapture {
    frame_rx: Receiver<Vec<u8>>,
    error: Arc<Mutex<Option<String>>>,
    active_child: Arc<Mutex<Option<Child>>>,
    stopping: Arc<AtomicBool>,
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
        if width == 0 || height == 0 || !fps.is_finite() || fps <= 0.0 {
            return Err("Webcam dimensions and frame rate must be positive".to_string());
        }
        let frame_size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(3))
            .ok_or_else(|| "Webcam output dimensions are too large".to_string())?;
        let (frame_tx, frame_rx) = sync_channel::<Vec<u8>>(2);
        let error = Arc::new(Mutex::new(None));
        let active_child = Arc::new(Mutex::new(None));
        let stopping = Arc::new(AtomicBool::new(false));

        let mut child = Command::new(ffmpeg_path)
            .arg("-nostdin")
            .arg("-loglevel")
            .arg("error")
            .arg("-f")
            .arg("dshow")
            .arg("-rtbufsize")
            .arg("256M")
            .arg("-i")
            .arg(format!("video={device_name}"))
            .arg("-an")
            .arg("-vf")
            .arg(format!("scale={width}:{height}"))
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgb24")
            .arg("-r")
            .arg(format!("{fps}"))
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start webcam capture: {error}"))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Webcam capture did not expose a frame pipe".to_string())?;
        *active_child
            .lock()
            .map_err(|_| "Webcam process state became unavailable".to_string())? = Some(child);

        let error_for_thread = Arc::clone(&error);
        let stopping_for_thread = Arc::clone(&stopping);
        thread::spawn(move || {
            let mut buffer = vec![0_u8; frame_size];
            loop {
                match stdout.read_exact(&mut buffer) {
                    Ok(()) => try_send_frame(&frame_tx, buffer.clone()),
                    Err(read_error) => {
                        if !stopping_for_thread.load(Ordering::Acquire)
                            && let Ok(mut error) = error_for_thread.lock()
                        {
                            *error = Some(
                                if read_error.kind() == std::io::ErrorKind::UnexpectedEof {
                                    "Webcam stopped producing frames. Refresh devices and reconnect it."
                                    .to_string()
                                } else {
                                    format!("Webcam capture stopped: {read_error}")
                                },
                            );
                        }
                        return;
                    }
                }
            }
        });

        Ok(Self {
            frame_rx,
            error,
            active_child,
            stopping,
            width,
            height,
        })
    }

    pub fn poll(&self) -> WebcamUpdate {
        let mut latest_frame = None;
        for frame in self.frame_rx.try_iter() {
            latest_frame = Some(frame);
        }
        let error = self.error.lock().ok().and_then(|mut error| error.take());
        WebcamUpdate {
            latest_frame,
            error,
        }
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for WebcamCapture {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(mut slot) = self.active_child.lock()
            && let Some(mut child) = slot.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn list_webcams(ffmpeg_path: &Path) -> Result<Vec<String>, String> {
    let output = Command::new(ffmpeg_path)
        .arg("-hide_banner")
        .arg("-f")
        .arg("dshow")
        .arg("-list_devices")
        .arg("true")
        .arg("-i")
        .arg("dummy")
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .output()
        .map_err(|error| format!("Could not query webcams with FFmpeg: {error}"))?;
    Ok(parse_directshow_devices(&String::from_utf8_lossy(
        &output.stderr,
    )))
}

fn parse_directshow_devices(output: &str) -> Vec<String> {
    let mut devices = Vec::new();
    let mut parsing_video = false;

    for line in output.lines() {
        if line.contains("DirectShow video devices") {
            parsing_video = true;
            continue;
        }
        if line.contains("DirectShow audio devices") {
            parsing_video = false;
            continue;
        }
        if !parsing_video || line.contains("Alternative name") {
            continue;
        }

        let Some(start) = line.find('"') else {
            continue;
        };
        let Some(end) = line[start + 1..].find('"') else {
            continue;
        };
        let name = &line[start + 1..start + 1 + end];
        if !name.is_empty() && !devices.iter().any(|device| device == name) {
            devices.push(name.to_string());
        }
    }
    devices
}

fn try_send_frame(sender: &SyncSender<Vec<u8>>, frame: Vec<u8>) {
    match sender.try_send(frame) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_returns_video_devices_without_alternative_names_or_duplicates() {
        let output = r#"
[dshow @ 0001] "Integrated Camera" (video)
[dshow @ 0001]   Alternative name "@device_pnp_long_name"
[dshow @ 0001] "USB Camera" (video)
[dshow @ 0001] "Integrated Camera" (video)
[dshow @ 0001] DirectShow audio devices (some may be both video and audio devices)
[dshow @ 0001] "Microphone" (audio)
"#;
        let output = format!(
            "[dshow @ 0001] DirectShow video devices (some may be both video and audio devices)\n{output}"
        );

        assert_eq!(
            parse_directshow_devices(&output),
            vec!["Integrated Camera", "USB Camera"]
        );
    }

    #[test]
    fn parser_handles_empty_output() {
        assert!(parse_directshow_devices("").is_empty());
    }

    #[test]
    fn invalid_dimensions_are_rejected_before_spawning() {
        let error = WebcamCapture::new(Path::new("ffmpeg"), "camera", 0, 240, 30.0)
            .err()
            .expect("invalid dimensions must fail");
        assert!(error.contains("positive"));
    }
}
