use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct MediaMetadata {
    pub duration: Duration,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
}

/// Commands that can be sent to the background decoder thread
pub enum DecoderCommand {
    Play,
    Pause,
    Seek(Duration),
    Stop,
}

/// Decoded frame data
pub struct DecodedFrame {
    pub data: Vec<u8>,
    pub timestamp: Duration,
}

pub struct MediaDecoder {
    cmd_tx: Sender<DecoderCommand>,
    frame_rx: Receiver<DecodedFrame>,
    metadata: MediaMetadata,
    is_paused: Arc<Mutex<bool>>,
    current_time: Arc<Mutex<Duration>>,
}

impl MediaDecoder {
    pub fn new(
        ffmpeg_path: &Path,
        file_path: &Path,
        target_width: u32,
        target_height: u32,
        target_fps: f32,
    ) -> Result<Self, String> {
        let metadata = get_metadata(ffmpeg_path, file_path)?;
        
        let (cmd_tx, cmd_rx) = channel::<DecoderCommand>();
        let (frame_tx, frame_rx) = channel::<DecodedFrame>();
        
        let is_paused = Arc::new(Mutex::new(false));
        let current_time = Arc::new(Mutex::new(Duration::ZERO));
        
        let is_paused_clone = is_paused.clone();
        let current_time_clone = current_time.clone();
        
        let ffmpeg_path_buf = ffmpeg_path.to_path_buf();
        let file_path_buf = file_path.to_path_buf();
        let duration = metadata.duration;

        thread::spawn(move || {
            let mut child: Option<Child> = None;
            let mut play_time = Duration::ZERO;
            let mut paused = false;
            let mut seek_request: Option<Duration> = None;
            
            // Frame size in bytes (RGB24)
            let frame_size = (target_width * target_height * 3) as usize;
            
            // We use standard frame interval
            let frame_interval = Duration::from_secs_f32(1.0 / target_fps);
            let mut last_frame_time = Instant::now();

            loop {
                // Check for incoming commands
                while let Ok(cmd) = cmd_rx.try_recv() {
                    match cmd {
                        DecoderCommand::Play => {
                            paused = false;
                            if let Ok(mut lock) = is_paused_clone.lock() {
                                *lock = false;
                            }
                        }
                        DecoderCommand::Pause => {
                            paused = true;
                            if let Ok(mut lock) = is_paused_clone.lock() {
                                *lock = true;
                            }
                        }
                        DecoderCommand::Seek(time) => {
                            seek_request = Some(time);
                        }
                        DecoderCommand::Stop => {
                            if let Some(mut c) = child.take() {
                                let _ = c.kill();
                            }
                            return;
                        }
                    }
                }

                // If seeking, restart the ffmpeg process at the new position
                if let Some(seek_time) = seek_request.take() {
                    if let Some(mut c) = child.take() {
                        let _ = c.kill();
                    }
                    play_time = seek_time;
                    if let Ok(mut lock) = current_time_clone.lock() {
                        *lock = play_time;
                    }
                }

                if paused {
                    thread::sleep(Duration::from_millis(50));
                    last_frame_time = Instant::now();
                    continue;
                }

                // Ensure child process is running
                if child.is_none() {
                    let scale_filter = format!(
                        "scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2:color=black",
                        target_width, target_height, target_width, target_height
                    );
                    
                    let mut cmd = Command::new(&ffmpeg_path_buf);
                    cmd.arg("-ss")
                       .arg(format!("{:.3}", play_time.as_secs_f64()))
                       .arg("-i")
                       .arg(&file_path_buf)
                       .arg("-vf")
                       .arg(&scale_filter)
                       .arg("-f")
                       .arg("rawvideo")
                       .arg("-pix_fmt")
                       .arg("rgb24")
                       .arg("-r")
                       .arg(format!("{}", target_fps))
                       .arg("-")
                       .stdout(Stdio::piped())
                       .stderr(Stdio::null());
                       
                    match cmd.spawn() {
                        Ok(c) => child = Some(c),
                        Err(e) => {
                            println!("Failed to spawn ffmpeg: {}", e);
                            thread::sleep(Duration::from_secs(1));
                            continue;
                        }
                    }
                    
                    last_frame_time = Instant::now();
                }

                // Read exactly one frame from the pipe
                if let Some(c) = child.as_mut() {
                    if let Some(stdout) = c.stdout.as_mut() {
                        let mut buffer = vec![0u8; frame_size];
                        
                        use std::io::Read;
                        match stdout.read_exact(&mut buffer) {
                            Ok(_) => {
                                // Frame successfully decoded
                                let _ = frame_tx.send(DecodedFrame {
                                    data: buffer,
                                    timestamp: play_time,
                                });
                                
                                // Advance play time
                                play_time += frame_interval;
                                if play_time >= duration {
                                    // Loop video
                                    play_time = Duration::ZERO;
                                    if let Some(mut c) = child.take() {
                                        let _ = c.kill();
                                    }
                                }
                                
                                if let Ok(mut lock) = current_time_clone.lock() {
                                    *lock = play_time;
                                }

                                // Throttle to maintain target frame rate
                                let elapsed = last_frame_time.elapsed();
                                if elapsed < frame_interval {
                                    thread::sleep(frame_interval - elapsed);
                                }
                                last_frame_time = Instant::now();
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                                // End of stream, restart
                                child = None;
                                play_time = Duration::ZERO;
                            }
                            Err(e) => {
                                println!("Error reading frame: {}", e);
                                child = None;
                                thread::sleep(Duration::from_millis(100));
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            cmd_tx,
            frame_rx,
            metadata,
            is_paused,
            current_time,
        })
    }

    pub fn play(&self) {
        let _ = self.cmd_tx.send(DecoderCommand::Play);
    }

    pub fn pause(&self) {
        let _ = self.cmd_tx.send(DecoderCommand::Pause);
    }

    pub fn seek(&self, time: Duration) {
        let _ = self.cmd_tx.send(DecoderCommand::Seek(time));
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.send(DecoderCommand::Stop);
    }

    pub fn next_frame(&self) -> Option<DecodedFrame> {
        self.frame_rx.try_recv().ok()
    }

    pub fn metadata(&self) -> &MediaMetadata {
        &self.metadata
    }

    pub fn is_paused(&self) -> bool {
        *self.is_paused.lock().unwrap()
    }

    pub fn current_time(&self) -> Duration {
        *self.current_time.lock().unwrap()
    }
}

/// Parses video metadata by spawning FFmpeg to inspect the file
pub fn get_metadata(ffmpeg_path: &Path, file_path: &Path) -> Result<MediaMetadata, String> {
    let output = Command::new(ffmpeg_path)
        .arg("-i")
        .arg(file_path)
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .output()
        .map_err(|e| format!("Failed to run FFmpeg: {}", e))?;
        
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    
    // Parse duration
    // Standard format in stderr: "Duration: 00:01:23.45"
    let duration = if let Some(idx) = stderr_str.find("Duration: ") {
        let sub = &stderr_str[idx + 10..];
        let parts: Vec<&str> = sub.split(',').next().unwrap_or("").trim().split(':').collect();
        if parts.len() == 3 {
            let hours = parts[0].parse::<u64>().unwrap_or(0);
            let minutes = parts[1].parse::<u64>().unwrap_or(0);
            let secs_parts: Vec<&str> = parts[2].split('.').collect();
            let secs = secs_parts[0].parse::<u64>().unwrap_or(0);
            let millis = if secs_parts.len() > 1 {
                let ms_str = secs_parts[1];
                let padded = format!("{:0<3}", ms_str);
                padded[..3].parse::<u64>().unwrap_or(0)
            } else {
                0
            };
            Duration::from_secs(hours * 3600 + minutes * 60 + secs) + Duration::from_millis(millis)
        } else {
            // Default fallback for images
            Duration::from_secs(3600 * 24) // 24 hours for static image
        }
    } else {
        Duration::from_secs(3600 * 24)
    };
    
    // Parse dimensions (width x height)
    // E.g., "1920x1080"
    let mut width = 1280;
    let mut height = 720;
    
    // Simple parsing logic: search for " [SAR " or "x" in video streams
    // An robust way is to scan for pattern like ", 1920x1080"
    for line in stderr_str.lines() {
        if line.contains("Stream #") && line.contains("Video:") {
            // Find parts with digits x digits
            let tokens: Vec<&str> = line.split(',').collect();
            for token in tokens {
                let trimmed = token.trim();
                let sub_parts: Vec<&str> = trimmed.split_whitespace().collect();
                for sp in sub_parts {
                    let dims: Vec<&str> = sp.split('x').collect();
                    if dims.len() == 2 {
                        if let (Ok(w), Ok(h)) = (dims[0].parse::<u32>(), dims[1].parse::<u32>()) {
                            // Verify realistic resolution
                            if w > 0 && h > 0 && w < 10000 && h < 10000 {
                                width = w;
                                height = h;
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Parse FPS
    let mut fps = 30.0;
    for line in stderr_str.lines() {
        if line.contains("Stream #") && line.contains("Video:") {
            if let Some(fps_idx) = line.find(" fps") {
                let start = line[..fps_idx].rfind(',').unwrap_or(0);
                let fps_str = line[start + 1..fps_idx].trim();
                if let Ok(f) = fps_str.parse::<f32>() {
                    fps = f;
                }
            }
        }
    }
    
    Ok(MediaMetadata {
        duration,
        width,
        height,
        fps,
    })
}
