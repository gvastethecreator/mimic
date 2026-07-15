use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct MediaMetadata {
    pub duration: Duration,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
}

#[derive(Debug)]
pub enum DecoderCommand {
    Play,
    Pause,
    Seek(Duration),
    Stop,
}

#[derive(Debug)]
enum DecoderEvent {
    Frame(Vec<u8>),
    Ended,
    Failed(String),
}

#[derive(Debug, Default)]
pub struct DecoderUpdate {
    pub latest_frame: Option<Vec<u8>>,
    pub ended: bool,
    pub error: Option<String>,
}

pub struct MediaDecoder {
    command_tx: std::sync::mpsc::Sender<DecoderCommand>,
    event_rx: Receiver<DecoderEvent>,
    metadata: MediaMetadata,
    is_paused: Arc<AtomicBool>,
    is_finished: Arc<AtomicBool>,
    current_time: Arc<Mutex<Duration>>,
    active_child: Arc<Mutex<Option<Child>>>,
}

impl MediaDecoder {
    pub fn new(
        ffmpeg_path: &Path,
        file_path: &Path,
        target_width: u32,
        target_height: u32,
        target_fps: f32,
    ) -> Result<Self, String> {
        if target_width == 0 || target_height == 0 || !target_fps.is_finite() || target_fps <= 0.0 {
            return Err("Decoder output dimensions and frame rate must be positive".to_string());
        }
        let frame_size = rgb_frame_size(target_width, target_height)?;
        let metadata = get_metadata(ffmpeg_path, file_path)?;
        let (command_tx, command_rx) = channel::<DecoderCommand>();
        let (event_tx, event_rx) = sync_channel::<DecoderEvent>(2);
        let is_paused = Arc::new(AtomicBool::new(false));
        let is_finished = Arc::new(AtomicBool::new(false));
        let current_time = Arc::new(Mutex::new(Duration::ZERO));
        let active_child = Arc::new(Mutex::new(None));

        let paused_for_thread = Arc::clone(&is_paused);
        let finished_for_thread = Arc::clone(&is_finished);
        let time_for_thread = Arc::clone(&current_time);
        let child_for_thread = Arc::clone(&active_child);
        let ffmpeg_path = ffmpeg_path.to_path_buf();
        let file_path = file_path.to_path_buf();
        let duration = metadata.duration;

        thread::spawn(move || {
            let frame_interval = Duration::from_secs_f32(1.0 / target_fps);
            let mut play_time = Duration::ZERO;
            let mut paused = false;
            let mut stdout: Option<ChildStdout> = None;
            let mut frame_buffer = vec![0_u8; frame_size];
            let mut last_frame_time = Instant::now();

            loop {
                while let Ok(command) = command_rx.try_recv() {
                    match command {
                        DecoderCommand::Play => {
                            paused = false;
                            paused_for_thread.store(false, Ordering::Release);
                        }
                        DecoderCommand::Pause => {
                            paused = true;
                            paused_for_thread.store(true, Ordering::Release);
                        }
                        DecoderCommand::Seek(requested) => {
                            kill_active_child(&child_for_thread);
                            stdout = None;
                            play_time = requested.min(duration);
                            set_current_time(&time_for_thread, play_time);
                            finished_for_thread.store(false, Ordering::Release);
                        }
                        DecoderCommand::Stop => {
                            kill_active_child(&child_for_thread);
                            return;
                        }
                    }
                }

                if paused {
                    thread::sleep(Duration::from_millis(20));
                    last_frame_time = Instant::now();
                    continue;
                }

                if stdout.is_none() {
                    match spawn_decoder(
                        &ffmpeg_path,
                        &file_path,
                        target_width,
                        target_height,
                        target_fps,
                        play_time,
                        &child_for_thread,
                    ) {
                        Ok(process_stdout) => stdout = Some(process_stdout),
                        Err(error) => {
                            send_terminal_event(&event_tx, DecoderEvent::Failed(error));
                            finished_for_thread.store(true, Ordering::Release);
                            return;
                        }
                    }
                    last_frame_time = Instant::now();
                }

                let read_result = stdout
                    .as_mut()
                    .expect("stdout is initialized above")
                    .read_exact(&mut frame_buffer);
                match read_result {
                    Ok(()) => {
                        try_send_frame(&event_tx, frame_buffer.clone());
                        play_time = (play_time + frame_interval).min(duration);
                        set_current_time(&time_for_thread, play_time);

                        if play_time >= duration {
                            kill_active_child(&child_for_thread);
                            send_terminal_event(&event_tx, DecoderEvent::Ended);
                            paused_for_thread.store(true, Ordering::Release);
                            finished_for_thread.store(true, Ordering::Release);
                            return;
                        }

                        let elapsed = last_frame_time.elapsed();
                        if elapsed < frame_interval {
                            thread::sleep(frame_interval - elapsed);
                        }
                        last_frame_time = Instant::now();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                        kill_active_child(&child_for_thread);
                        send_terminal_event(&event_tx, DecoderEvent::Ended);
                        paused_for_thread.store(true, Ordering::Release);
                        finished_for_thread.store(true, Ordering::Release);
                        return;
                    }
                    Err(error) => {
                        kill_active_child(&child_for_thread);
                        send_terminal_event(
                            &event_tx,
                            DecoderEvent::Failed(format!("Media decoding stopped: {error}")),
                        );
                        finished_for_thread.store(true, Ordering::Release);
                        return;
                    }
                }
            }
        });

        Ok(Self {
            command_tx,
            event_rx,
            metadata,
            is_paused,
            is_finished,
            current_time,
            active_child,
        })
    }

    pub fn play(&self) {
        let _ = self.command_tx.send(DecoderCommand::Play);
    }

    pub fn pause(&self) {
        let _ = self.command_tx.send(DecoderCommand::Pause);
    }

    pub fn seek(&self, time: Duration) {
        let _ = self.command_tx.send(DecoderCommand::Seek(time));
    }

    pub fn stop(self) {
        drop(self);
    }

    pub fn poll(&self) -> DecoderUpdate {
        let mut update = DecoderUpdate::default();
        for event in self.event_rx.try_iter() {
            match event {
                DecoderEvent::Frame(frame) => update.latest_frame = Some(frame),
                DecoderEvent::Ended => update.ended = true,
                DecoderEvent::Failed(error) => update.error = Some(error),
            }
        }
        update
    }

    pub fn metadata(&self) -> &MediaMetadata {
        &self.metadata
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Acquire)
    }

    pub fn is_finished(&self) -> bool {
        self.is_finished.load(Ordering::Acquire)
    }

    pub fn current_time(&self) -> Duration {
        self.current_time
            .lock()
            .map(|time| *time)
            .unwrap_or(Duration::ZERO)
    }
}

impl Drop for MediaDecoder {
    fn drop(&mut self) {
        let _ = self.command_tx.send(DecoderCommand::Stop);
        kill_active_child(&self.active_child);
    }
}

pub fn get_metadata(ffmpeg_path: &Path, file_path: &Path) -> Result<MediaMetadata, String> {
    let output = Command::new(ffmpeg_path)
        .arg("-hide_banner")
        .arg("-i")
        .arg(file_path)
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .output()
        .map_err(|error| format!("Could not inspect media with FFmpeg: {error}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_metadata(&stderr).map_err(|error| format!("{} ({})", error, file_path.to_string_lossy()))
}

fn parse_metadata(ffmpeg_output: &str) -> Result<MediaMetadata, String> {
    let video_line = ffmpeg_output
        .lines()
        .find(|line| line.contains("Stream #") && line.contains("Video:"))
        .ok_or_else(|| "FFmpeg found no video stream".to_string())?;

    let (width, height) = parse_dimensions(video_line)
        .ok_or_else(|| "FFmpeg did not report valid video dimensions".to_string())?;
    let fps = parse_fps(video_line).unwrap_or(30.0);
    let duration = ffmpeg_output
        .find("Duration:")
        .and_then(|index| {
            ffmpeg_output[index + "Duration:".len()..]
                .split(',')
                .next()
                .and_then(parse_duration)
        })
        .unwrap_or_else(|| Duration::from_secs(24 * 60 * 60));

    Ok(MediaMetadata {
        duration,
        width,
        height,
        fps,
    })
}

fn parse_dimensions(line: &str) -> Option<(u32, u32)> {
    line.split(|character: char| character.is_whitespace() || character == ',')
        .filter_map(|token| token.split_once('x'))
        .find_map(|(width, height)| {
            let width = width.parse::<u32>().ok()?;
            let height = height
                .trim_matches(|character: char| !character.is_ascii_digit())
                .parse::<u32>()
                .ok()?;
            (width > 0 && height > 0 && width <= 16_384 && height <= 16_384)
                .then_some((width, height))
        })
}

fn parse_fps(line: &str) -> Option<f32> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    tokens.windows(2).find_map(|pair| {
        pair[1]
            .trim_matches(',')
            .eq_ignore_ascii_case("fps")
            .then(|| pair[0].trim_matches(','))
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|fps| fps.is_finite() && *fps > 0.0 && *fps <= 240.0)
    })
}

fn parse_duration(value: &str) -> Option<Duration> {
    let mut parts = value.trim().split(':');
    let hours = parts.next()?.parse::<u64>().ok()?;
    let minutes = parts.next()?.parse::<u64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    if parts.next().is_some() || minutes >= 60 || !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(Duration::from_secs(hours * 3600 + minutes * 60) + Duration::from_secs_f64(seconds))
}

#[allow(clippy::too_many_arguments)]
fn spawn_decoder(
    ffmpeg_path: &Path,
    file_path: &Path,
    width: u32,
    height: u32,
    fps: f32,
    start_time: Duration,
    active_child: &Arc<Mutex<Option<Child>>>,
) -> Result<ChildStdout, String> {
    let scale_filter = format!(
        "scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black"
    );
    let mut command = Command::new(ffmpeg_path);
    command.arg("-nostdin").arg("-loglevel").arg("error");
    if is_still_image(file_path) {
        command
            .arg("-loop")
            .arg("1")
            .arg("-framerate")
            .arg(format!("{fps}"));
    } else {
        command
            .arg("-ss")
            .arg(format!("{:.3}", start_time.as_secs_f64()));
    }
    let mut child = command
        .arg("-i")
        .arg(file_path)
        .arg("-vf")
        .arg(scale_filter)
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
        .map_err(|error| format!("Could not start FFmpeg decoder: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "FFmpeg decoder did not expose a frame pipe".to_string())?;
    *active_child
        .lock()
        .map_err(|_| "Decoder process state became unavailable".to_string())? = Some(child);
    Ok(stdout)
}

fn is_still_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["png", "jpg", "jpeg"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

fn rgb_frame_size(width: u32, height: u32) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| "Decoder output dimensions are too large".to_string())
}

fn set_current_time(current_time: &Arc<Mutex<Duration>>, value: Duration) {
    if let Ok(mut current_time) = current_time.lock() {
        *current_time = value;
    }
}

fn kill_active_child(active_child: &Arc<Mutex<Option<Child>>>) {
    if let Ok(mut slot) = active_child.lock()
        && let Some(mut child) = slot.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn try_send_frame(sender: &SyncSender<DecoderEvent>, frame: Vec<u8>) {
    match sender.try_send(DecoderEvent::Frame(frame)) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}

fn send_terminal_event(sender: &SyncSender<DecoderEvent>, event: DecoderEvent) {
    let _ = sender.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'sample.mp4':
  Duration: 01:02:03.45, start: 0.000000, bitrate: 1200 kb/s
  Stream #0:0: Video: h264 (High), yuv420p(progressive), 1920x1080 [SAR 1:1 DAR 16:9], 29.97 fps, 30 tbr
"#;

    #[test]
    fn metadata_parser_extracts_duration_dimensions_and_fps() {
        let metadata = parse_metadata(SAMPLE).unwrap();
        assert_eq!(metadata.width, 1920);
        assert_eq!(metadata.height, 1080);
        assert!((metadata.fps - 29.97).abs() < f32::EPSILON);
        assert_eq!(metadata.duration, Duration::from_millis(3_723_450));
    }

    #[test]
    fn metadata_parser_rejects_non_video_output() {
        let error = parse_metadata("sample.txt: Invalid data found when processing input")
            .expect_err("invalid input must be rejected");
        assert!(error.contains("no video stream"));
    }

    #[test]
    fn still_image_metadata_uses_long_preview_duration() {
        let output = "Stream #0:0: Video: png, rgba, 800x600, 25 fps";
        let metadata = parse_metadata(output).unwrap();
        assert_eq!(metadata.duration, Duration::from_secs(24 * 60 * 60));
    }

    #[test]
    fn still_image_detection_is_case_insensitive_and_excludes_animation() {
        assert!(is_still_image(Path::new("scene.PNG")));
        assert!(is_still_image(Path::new("scene.jpeg")));
        assert!(!is_still_image(Path::new("scene.gif")));
        assert!(!is_still_image(Path::new("scene.mp4")));
    }

    #[test]
    fn frame_size_rejects_overflow() {
        assert!(rgb_frame_size(u32::MAX, u32::MAX).is_err());
    }
}
