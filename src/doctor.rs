use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::compositor::{FrameCompositor, PipPosition};
use crate::decoder::MediaDecoder;
use crate::setup;
use crate::webcam::{self, WebcamCapture};

pub const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofStatus {
    Pass,
    Invalid,
    Unavailable,
    Failed,
}

impl ProofStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Invalid => "INVALID",
            Self::Unavailable => "UNAVAILABLE",
            Self::Failed => "FAILED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub status: ProofStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub command: String,
    pub status: ProofStatus,
    pub summary: String,
    pub duration_ms: u64,
    pub checks: Vec<CheckResult>,
    pub details: BTreeMap<String, Value>,
}

impl DoctorReport {
    fn new(command: &str, summary: impl Into<String>) -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            command: command.to_string(),
            status: ProofStatus::Pass,
            summary: summary.into(),
            duration_ms: 0,
            checks: Vec::new(),
            details: BTreeMap::new(),
        }
    }

    fn finish(mut self, started: Instant) -> Self {
        self.duration_ms = elapsed_ms(started.elapsed());
        self
    }

    fn add_check(
        &mut self,
        name: impl Into<String>,
        status: ProofStatus,
        message: impl Into<String>,
    ) {
        self.checks.push(CheckResult {
            name: name.into(),
            status,
            message: message.into(),
        });
    }

    fn detail(&mut self, key: impl Into<String>, value: impl Serialize) {
        self.details.insert(
            key.into(),
            serde_json::to_value(value).expect("doctor detail values must serialize"),
        );
    }

    pub fn exit_code(&self) -> i32 {
        match self.status {
            ProofStatus::Pass => 0,
            ProofStatus::Invalid => 2,
            ProofStatus::Unavailable => 3,
            ProofStatus::Failed => 4,
        }
    }

    pub fn render_text(&self) -> String {
        let mut output = format!(
            "Mimic doctor / {}: {}\n{}\n",
            self.command,
            self.status.label(),
            self.summary
        );
        for check in &self.checks {
            output.push_str(&format!(
                "[{}] {}: {}\n",
                check.status.label(),
                check.name,
                check.message
            ));
        }
        output.push_str(&format!("Duration: {} ms\n", self.duration_ms));
        output
    }
}

#[derive(Debug, Clone)]
pub struct MediaProbeOptions {
    pub ffmpeg: Option<PathBuf>,
    pub input: PathBuf,
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct CameraProbeOptions {
    pub ffmpeg: Option<PathBuf>,
    pub device: String,
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct VirtualOutputProbeOptions {
    pub ffmpeg: Option<PathBuf>,
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct SoakOptions {
    pub ffmpeg: Option<PathBuf>,
    pub input: PathBuf,
    pub duration: Duration,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
}

#[derive(Debug, Clone, Copy)]
struct ProbeLimits {
    frames: u32,
    width: u32,
    height: u32,
    fps: f32,
    timeout: Duration,
}

pub fn run_check(ffmpeg_override: Option<PathBuf>) -> DoctorReport {
    let started = Instant::now();
    let mut report = DoctorReport::new("check", "Required runtime dependencies are available.");
    report.detail("mimic_version", env!("CARGO_PKG_VERSION"));
    report.detail("os", std::env::consts::OS);
    report.detail("architecture", std::env::consts::ARCH);
    report.detail(
        "app_data_directory",
        setup::app_dir_path().to_string_lossy(),
    );

    let ffmpeg = match resolve_ffmpeg(ffmpeg_override.as_deref()) {
        Ok(path) => {
            let version = ffmpeg_version(&path).unwrap_or_else(|| "version unavailable".into());
            report.add_check(
                "ffmpeg",
                ProofStatus::Pass,
                format!("{} ({version})", path.display()),
            );
            report.detail("ffmpeg_path", path.to_string_lossy());
            Some(path)
        }
        Err(error) => {
            report.add_check("ffmpeg", ProofStatus::Unavailable, error);
            None
        }
    };

    let backends = setup::available_virtual_camera_backends();
    let backend_labels = backends
        .iter()
        .map(|backend| backend.label())
        .collect::<Vec<_>>();
    if backend_labels.is_empty() {
        report.add_check(
            "virtual_camera",
            ProofStatus::Unavailable,
            "No OBS or Unity Capture backend is registered.",
        );
    } else {
        report.add_check(
            "virtual_camera",
            ProofStatus::Pass,
            backend_labels.join(", "),
        );
    }
    report.detail("virtual_camera_backends", &backend_labels);

    if let Some(ffmpeg) = ffmpeg.as_deref() {
        match webcam::list_webcams(ffmpeg) {
            Ok(devices) => {
                report.add_check(
                    "directshow_devices",
                    ProofStatus::Pass,
                    format!(
                        "{} video device(s) detected; no device was opened.",
                        devices.len()
                    ),
                );
                report.detail("directshow_devices", devices);
            }
            Err(error) => report.add_check("directshow_devices", ProofStatus::Failed, error),
        }
    }

    if ffmpeg.is_none() || backend_labels.is_empty() {
        report.status = ProofStatus::Unavailable;
        report.summary =
            "Mimic is not ready: one or more required dependencies are unavailable.".to_string();
    } else if report
        .checks
        .iter()
        .any(|check| check.status == ProofStatus::Failed)
    {
        report.status = ProofStatus::Failed;
        report.summary = "Runtime discovery returned an unexpected failure.".to_string();
    }

    report.finish(started)
}

pub fn run_media_probe(options: &MediaProbeOptions) -> DoctorReport {
    let started = Instant::now();
    let mut report = DoctorReport::new("media", "Media decoding proof passed.");
    let Some(ffmpeg) = prepare_probe(
        &mut report,
        options.ffmpeg.as_deref(),
        Some(&options.input),
        ProbeLimits {
            frames: options.frames,
            width: options.width,
            height: options.height,
            fps: options.fps,
            timeout: options.timeout,
        },
    ) else {
        return report.finish(started);
    };

    let decoder = match MediaDecoder::new(
        &ffmpeg,
        &options.input,
        options.width,
        options.height,
        options.fps,
    ) {
        Ok(decoder) => decoder,
        Err(error) => {
            report.status = ProofStatus::Failed;
            report.summary = "FFmpeg could not start the requested media proof.".to_string();
            report.add_check("decoder", ProofStatus::Failed, error);
            return report.finish(started);
        }
    };
    report.detail("source_width", decoder.metadata().width);
    report.detail("source_height", decoder.metadata().height);
    report.detail("source_fps", decoder.metadata().fps);
    report.detail(
        "source_duration_ms",
        elapsed_ms(decoder.metadata().duration),
    );

    let deadline = Instant::now() + options.timeout;
    let mut frames = 0_u32;
    let mut hash = Sha256::new();
    let mut terminal_error = None;
    while frames < options.frames && Instant::now() < deadline {
        let update = decoder.poll();
        if let Some(frame) = update.latest_frame {
            hash.update(&frame);
            frames += 1;
        }
        if let Some(error) = update.error {
            terminal_error = Some(error);
            break;
        }
        if update.ended && frames < options.frames {
            terminal_error = Some(format!(
                "Media ended after {frames} frame(s), before the requested {}.",
                options.frames
            ));
            break;
        }
        thread::sleep(Duration::from_millis(4));
    }
    drop(decoder);

    report.detail("frames_received", frames);
    report.detail("frame_sha256", digest_hex(hash.finalize().as_ref()));
    if let Some(error) = terminal_error {
        report.status = ProofStatus::Failed;
        report.summary = "Media decoding stopped before the requested proof completed.".into();
        report.add_check("frames", ProofStatus::Failed, error);
    } else if frames < options.frames {
        report.status = ProofStatus::Failed;
        report.summary = "Media decoding timed out.".into();
        report.add_check(
            "frames",
            ProofStatus::Failed,
            format!("Received {frames}/{} frame(s).", options.frames),
        );
    } else {
        report.add_check(
            "frames",
            ProofStatus::Pass,
            format!("Received {frames} bounded RGB frame(s)."),
        );
    }
    report.finish(started)
}

pub fn run_camera_probe(options: &CameraProbeOptions) -> DoctorReport {
    let started = Instant::now();
    let mut report = DoctorReport::new("camera", "Physical-camera frame proof passed.");
    let Some(ffmpeg) = prepare_probe(
        &mut report,
        options.ffmpeg.as_deref(),
        None,
        ProbeLimits {
            frames: options.frames,
            width: options.width,
            height: options.height,
            fps: options.fps,
            timeout: options.timeout,
        },
    ) else {
        return report.finish(started);
    };
    if options.device.trim().is_empty() {
        invalidate(&mut report, "device", "--device must not be empty.");
        return report.finish(started);
    }

    let capture = match WebcamCapture::new(
        &ffmpeg,
        &options.device,
        options.width,
        options.height,
        options.fps,
    ) {
        Ok(capture) => capture,
        Err(error) => {
            report.status = ProofStatus::Unavailable;
            report.summary = "The requested physical camera could not be opened.".into();
            report.add_check("camera", ProofStatus::Unavailable, error);
            return report.finish(started);
        }
    };

    let deadline = Instant::now() + options.timeout;
    let mut frames = 0_u32;
    let mut hash = Sha256::new();
    let mut terminal_error = None;
    while frames < options.frames && Instant::now() < deadline {
        let update = capture.poll();
        if let Some(frame) = update.latest_frame {
            hash.update(&frame);
            frames += 1;
        }
        if let Some(error) = update.error {
            terminal_error = Some(error);
            break;
        }
        thread::sleep(Duration::from_millis(4));
    }
    capture.stop();

    report.detail("device", &options.device);
    report.detail("width", options.width);
    report.detail("height", options.height);
    report.detail("frames_received", frames);
    report.detail(
        "aggregate_frame_sha256",
        digest_hex(hash.finalize().as_ref()),
    );
    report.detail("image_payload_retained", false);

    if let Some(error) = terminal_error {
        report.status = ProofStatus::Failed;
        report.summary = "The physical camera stopped before the proof completed.".into();
        report.add_check("frames", ProofStatus::Failed, error);
    } else if frames < options.frames {
        report.status = ProofStatus::Failed;
        report.summary = "The physical-camera proof timed out.".into();
        report.add_check(
            "frames",
            ProofStatus::Failed,
            format!("Received {frames}/{} frame(s).", options.frames),
        );
    } else {
        report.add_check(
            "frames",
            ProofStatus::Pass,
            format!("Counted {frames} frame(s); only an aggregate hash was retained."),
        );
    }
    report.finish(started)
}

pub fn run_virtual_output_probe(options: &VirtualOutputProbeOptions) -> DoctorReport {
    let started = Instant::now();
    let mut report = DoctorReport::new(
        "virtual-output",
        "An independent FFmpeg receiver observed virtual-camera frames.",
    );
    let Some(ffmpeg) = prepare_probe(
        &mut report,
        options.ffmpeg.as_deref(),
        None,
        ProbeLimits {
            frames: options.frames,
            width: options.width,
            height: options.height,
            fps: options.fps,
            timeout: options.timeout,
        },
    ) else {
        return report.finish(started);
    };
    if setup::available_virtual_camera_backends().is_empty() {
        report.status = ProofStatus::Unavailable;
        report.summary = "No supported virtual-camera backend is registered.".into();
        report.add_check(
            "virtual_camera",
            ProofStatus::Unavailable,
            "Install OBS Virtual Camera or Unity Capture first.",
        );
        return report.finish(started);
    }

    let mut compositor = FrameCompositor::new(options.width, options.height, options.fps);
    let details = match compositor.init_camera() {
        Ok(details) => details,
        Err(error) => {
            report.status = ProofStatus::Unavailable;
            report.summary = "The virtual-camera sender could not initialize.".into();
            report.add_check("sender", ProofStatus::Unavailable, error);
            return report.finish(started);
        }
    };
    report.detail("backend", &details.backend);
    report.detail("device", &details.device);

    let frame_interval = Duration::from_secs_f32(1.0 / options.fps);
    let mut frames_sent = 0_u32;
    for _ in 0..3 {
        let frame = deterministic_frame(options.width, options.height, frames_sent);
        if let Err(error) = compositor.process_and_send(&frame, None, PipPosition::BottomRight, 0) {
            compositor.release();
            report.status = ProofStatus::Failed;
            report.summary = "The virtual-camera sender failed during warmup.".into();
            report.add_check("sender", ProofStatus::Failed, error);
            return report.finish(started);
        }
        frames_sent += 1;
        thread::sleep(frame_interval.min(Duration::from_millis(50)));
    }

    let input = format!("video={}", details.device);
    let mut receiver = match Command::new(&ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error"])
        .args(["-f", "dshow", "-rtbufsize", "256M", "-i"])
        .arg(input)
        .arg("-an")
        .args(["-frames:v", &options.frames.to_string()])
        .args(["-f", "framemd5", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            compositor.release();
            report.status = ProofStatus::Unavailable;
            report.summary = "The independent receiver could not start.".into();
            report.add_check("receiver", ProofStatus::Unavailable, error.to_string());
            return report.finish(started);
        }
    };

    let deadline = Instant::now() + options.timeout;
    let mut send_error = None;
    let mut receiver_exited = false;
    thread::sleep(Duration::from_millis(250));
    while Instant::now() < deadline {
        let frame = deterministic_frame(options.width, options.height, frames_sent);
        if let Err(error) = compositor.process_and_send(&frame, None, PipPosition::BottomRight, 0) {
            send_error = Some(error);
            break;
        }
        frames_sent += 1;
        match receiver.try_wait() {
            Ok(Some(_)) => {
                receiver_exited = true;
                break;
            }
            Ok(None) => {}
            Err(error) => {
                send_error = Some(format!("Could not inspect receiver state: {error}"));
                break;
            }
        }
        thread::sleep(frame_interval.min(Duration::from_millis(50)));
    }
    compositor.release();
    if !receiver_exited {
        let _ = receiver.kill();
    }
    let output = receiver.wait_with_output();

    report.detail("frames_sent", frames_sent);
    match (send_error, output) {
        (Some(error), _) => {
            report.status = ProofStatus::Failed;
            report.summary = "The virtual-camera sender failed during receiver proof.".into();
            report.add_check("sender", ProofStatus::Failed, error);
        }
        (_, Err(error)) => {
            report.status = ProofStatus::Failed;
            report.summary = "The receiver process could not be collected.".into();
            report.add_check("receiver", ProofStatus::Failed, error.to_string());
        }
        (None, Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let receiver_frames = stdout
                .lines()
                .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
                .count() as u32;
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            report.detail("receiver_frames", receiver_frames);
            report.detail("receiver_framemd5_sha256", sha256_bytes(&output.stdout));
            report.detail("receiver_exit_code", output.status.code());
            if output.status.success() && receiver_frames >= options.frames {
                report.add_check(
                    "receiver",
                    ProofStatus::Pass,
                    format!(
                        "FFmpeg received {receiver_frames} frame(s) from {} through {}.",
                        details.device, details.backend
                    ),
                );
            } else {
                report.status = ProofStatus::Failed;
                report.summary = "The receiver did not complete the requested frame proof.".into();
                report.add_check(
                    "receiver",
                    ProofStatus::Failed,
                    if stderr.is_empty() {
                        format!(
                            "Receiver exited {:?} after {receiver_frames}/{} frame(s).",
                            output.status.code(),
                            options.frames
                        )
                    } else {
                        stderr
                    },
                );
            }
        }
    }
    report.finish(started)
}

pub fn run_soak(options: &SoakOptions) -> DoctorReport {
    let started = Instant::now();
    let mut report = DoctorReport::new(
        "soak",
        "Bounded media soak stayed within its quality budget.",
    );
    if options.duration < Duration::from_secs(1) || options.duration > Duration::from_secs(3600) {
        invalidate(
            &mut report,
            "duration",
            "--seconds must be between 1 and 3600.",
        );
        return report.finish(started);
    }
    let Some(ffmpeg) = prepare_probe(
        &mut report,
        options.ffmpeg.as_deref(),
        Some(&options.input),
        ProbeLimits {
            frames: 1,
            width: options.width,
            height: options.height,
            fps: options.fps,
            timeout: options.duration + Duration::from_secs(10),
        },
    ) else {
        return report.finish(started);
    };

    let deadline = Instant::now() + options.duration;
    let memory_start = working_set_bytes();
    let mut memory_peak = memory_start.unwrap_or(0);
    let mut last_memory_sample = Instant::now();
    let mut last_frame_at = None;
    let mut max_frame_gap = Duration::ZERO;
    let mut frames = 0_u64;
    let mut loops = 0_u32;
    let mut hash = Sha256::new();
    let mut terminal_error = None;

    let mut decoder = match new_soak_decoder(options, &ffmpeg) {
        Ok(decoder) => Some(decoder),
        Err(error) => {
            report.status = ProofStatus::Failed;
            report.summary = "The soak decoder could not start.".into();
            report.add_check("decoder", ProofStatus::Failed, error);
            return report.finish(started);
        }
    };

    while Instant::now() < deadline {
        let update = decoder.as_ref().expect("soak decoder is present").poll();
        if let Some(frame) = update.latest_frame {
            let now = Instant::now();
            if let Some(previous) = last_frame_at {
                max_frame_gap = max_frame_gap.max(now.saturating_duration_since(previous));
            }
            last_frame_at = Some(now);
            hash.update(&frame);
            frames += 1;
        }
        if let Some(error) = update.error {
            terminal_error = Some(error);
            break;
        }
        if update.ended && Instant::now() < deadline {
            drop(decoder.take());
            loops += 1;
            match new_soak_decoder(options, &ffmpeg) {
                Ok(next) => decoder = Some(next),
                Err(error) => {
                    terminal_error = Some(error);
                    break;
                }
            }
        }
        if last_memory_sample.elapsed() >= Duration::from_millis(250) {
            if let Some(memory) = working_set_bytes() {
                memory_peak = memory_peak.max(memory);
            }
            last_memory_sample = Instant::now();
        }
        thread::sleep(Duration::from_millis(4));
    }
    drop(decoder.take());
    let memory_end = working_set_bytes();

    let minimum_frames =
        ((options.duration.as_secs_f32() * options.fps * 0.5).floor() as u64).max(1);
    let memory_growth = memory_end
        .zip(memory_start)
        .map(|(end, start)| end.saturating_sub(start));
    report.detail("frames_received", frames);
    report.detail("minimum_frame_budget", minimum_frames);
    report.detail("media_loops", loops);
    report.detail("max_frame_gap_ms", elapsed_ms(max_frame_gap));
    report.detail(
        "aggregate_frame_sha256",
        digest_hex(hash.finalize().as_ref()),
    );
    report.detail("working_set_start_bytes", memory_start);
    report.detail("working_set_peak_bytes", memory_peak);
    report.detail("working_set_end_bytes", memory_end);
    report.detail("working_set_growth_bytes", memory_growth);
    report.detail(
        "decoder_cleanup",
        "drop completed; child kill/wait is synchronous",
    );

    if let Some(error) = terminal_error {
        report.status = ProofStatus::Failed;
        report.summary = "The bounded soak encountered a decoder failure.".into();
        report.add_check("decoder", ProofStatus::Failed, error);
    } else if frames < minimum_frames {
        report.status = ProofStatus::Failed;
        report.summary = "The bounded soak missed its frame-progress budget.".into();
        report.add_check(
            "frame_budget",
            ProofStatus::Failed,
            format!("Received {frames} frame(s); required at least {minimum_frames}."),
        );
    } else if max_frame_gap > Duration::from_secs(2) {
        report.status = ProofStatus::Failed;
        report.summary = "The bounded soak contained a frame stall longer than two seconds.".into();
        report.add_check(
            "frame_gap",
            ProofStatus::Failed,
            format!("Maximum observed gap was {} ms.", elapsed_ms(max_frame_gap)),
        );
    } else if memory_growth.is_some_and(|growth| growth > 256 * 1024 * 1024) {
        report.status = ProofStatus::Failed;
        report.summary = "The bounded soak exceeded its 256 MiB working-set growth budget.".into();
        report.add_check(
            "memory_budget",
            ProofStatus::Failed,
            format!("Working set grew by {} bytes.", memory_growth.unwrap()),
        );
    } else {
        report.add_check(
            "frame_budget",
            ProofStatus::Pass,
            format!("Received {frames} frame(s); minimum was {minimum_frames}."),
        );
        report.add_check(
            "cleanup",
            ProofStatus::Pass,
            "Decoder teardown synchronously killed and waited for its FFmpeg child.",
        );
    }
    report.finish(started)
}

fn new_soak_decoder(options: &SoakOptions, ffmpeg: &Path) -> Result<MediaDecoder, String> {
    MediaDecoder::new(
        ffmpeg,
        &options.input,
        options.width,
        options.height,
        options.fps,
    )
}

fn prepare_probe(
    report: &mut DoctorReport,
    ffmpeg_override: Option<&Path>,
    input: Option<&Path>,
    limits: ProbeLimits,
) -> Option<PathBuf> {
    let ProbeLimits {
        frames,
        width,
        height,
        fps,
        timeout,
    } = limits;
    if frames == 0 || frames > 10_000 {
        invalidate(report, "frames", "--frames must be between 1 and 10000.");
        return None;
    }
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        invalidate(
            report,
            "dimensions",
            "Width/height must be positive and no larger than 7680x4320.",
        );
        return None;
    }
    if !fps.is_finite() || !(1.0..=120.0).contains(&fps) {
        invalidate(report, "fps", "--fps must be between 1 and 120.");
        return None;
    }
    if timeout < Duration::from_secs(1) || timeout > Duration::from_secs(7200) {
        invalidate(
            report,
            "timeout",
            "Timeout must be between 1 and 7200 seconds.",
        );
        return None;
    }
    if let Some(input) = input
        && (!input.is_file())
    {
        invalidate(
            report,
            "input",
            format!("Input file does not exist: {}", input.display()),
        );
        return None;
    }
    match resolve_ffmpeg(ffmpeg_override) {
        Ok(path) => {
            report.detail("ffmpeg_path", path.to_string_lossy());
            Some(path)
        }
        Err(error) => {
            report.status = ProofStatus::Unavailable;
            report.summary = "FFmpeg is unavailable.".into();
            report.add_check("ffmpeg", ProofStatus::Unavailable, error);
            None
        }
    }
}

fn invalidate(report: &mut DoctorReport, name: &str, message: impl Into<String>) {
    report.status = ProofStatus::Invalid;
    report.summary = "The requested proof has invalid input.".into();
    report.add_check(name, ProofStatus::Invalid, message);
}

fn resolve_ffmpeg(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return if setup::validate_ffmpeg(path) {
            Ok(path.to_path_buf())
        } else {
            Err(format!(
                "The explicit FFmpeg path is not executable: {}",
                path.display()
            ))
        };
    }
    setup::get_ffmpeg_path().ok_or_else(|| {
        "FFmpeg was not found on PATH, beside the executable, or in Mimic's app-data directory."
            .to_string()
    })
}

fn ffmpeg_version(path: &Path) -> Option<String> {
    let output = Command::new(path)
        .args(["-hide_banner", "-version"])
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("unknown FFmpeg version")
            .trim()
            .to_string()
    })
}

fn deterministic_frame(width: u32, height: u32, index: u32) -> Vec<u8> {
    let mut frame = vec![0_u8; width as usize * height as usize * 3];
    for y in 0..height {
        for x in 0..width {
            let offset = ((y * width + x) * 3) as usize;
            frame[offset] = ((x + index * 7) % 256) as u8;
            frame[offset + 1] = ((y * 2 + index * 11) % 256) as u8;
            frame[offset + 2] = (((x / 8 + y / 8 + index) % 2) * 192 + 32) as u8;
        }
    }
    frame
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    digest_hex(hash.finalize().as_ref())
}

fn digest_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut value, "{byte:02x}").expect("writing to a String cannot fail");
    }
    value
}

fn elapsed_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(windows)]
fn working_set_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    let result = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    (result != 0).then_some(counters.WorkingSetSize as u64)
}

#[cfg(not(windows))]
fn working_set_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_frame_is_stable_and_changes_with_index() {
        let first = deterministic_frame(4, 3, 7);
        let same = deterministic_frame(4, 3, 7);
        let next = deterministic_frame(4, 3, 8);
        assert_eq!(first, same);
        assert_ne!(first, next);
        assert_eq!(first.len(), 4 * 3 * 3);
    }

    #[test]
    fn report_exit_codes_are_stable() {
        let mut report = DoctorReport::new("check", "ok");
        assert_eq!(report.exit_code(), 0);
        report.status = ProofStatus::Invalid;
        assert_eq!(report.exit_code(), 2);
        report.status = ProofStatus::Unavailable;
        assert_eq!(report.exit_code(), 3);
        report.status = ProofStatus::Failed;
        assert_eq!(report.exit_code(), 4);
    }

    #[test]
    fn text_report_is_plain_and_actionable() {
        let mut report = DoctorReport::new("media", "Media proof passed.");
        report.add_check("frames", ProofStatus::Pass, "3 frames received");
        let text = report.render_text();
        assert!(text.contains("Mimic doctor / media: PASS"));
        assert!(text.contains("[PASS] frames: 3 frames received"));
        assert!(!text.contains("\u{1b}["));
    }

    #[test]
    fn invalid_probe_shape_returns_usage_exit_code_without_spawning() {
        let options = MediaProbeOptions {
            ffmpeg: None,
            input: PathBuf::from("missing.mp4"),
            frames: 0,
            width: 0,
            height: 0,
            fps: 0.0,
            timeout: Duration::ZERO,
        };
        let report = run_media_probe(&options);
        assert_eq!(report.status, ProofStatus::Invalid);
        assert_eq!(report.exit_code(), 2);
    }
}
