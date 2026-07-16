use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use mimic::doctor::{
    CameraProbeOptions, DoctorReport, MediaProbeOptions, SoakOptions, VirtualOutputProbeOptions,
    run_camera_probe, run_check, run_media_probe, run_soak, run_virtual_output_probe,
};

#[derive(Debug, Parser)]
#[command(
    name = "mimic-doctor",
    version,
    about = "Verify Mimic's local Windows runtime and media paths.",
    long_about = "Verify Mimic's local Windows runtime and media paths without opening the GUI. Hardware and virtual-output commands are always explicit and bounded.",
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// Emit one stable JSON document to stdout.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: DoctorCommand,
}

#[derive(Debug, Subcommand)]
enum DoctorCommand {
    /// Inspect FFmpeg, DirectShow devices, and virtual-camera backends without opening a device.
    Check(RuntimeArgs),

    /// Decode a bounded number of frames from an explicit media file.
    Media(MediaArgs),

    /// Open an explicit physical camera and retain only frame counts plus an aggregate hash.
    Camera(CameraArgs),

    /// Send deterministic frames and prove that an independent FFmpeg receiver observes them.
    VirtualOutput(VirtualOutputArgs),

    /// Run a bounded media loop and report progress, stalls, memory, and cleanup.
    Soak(SoakArgs),
}

#[derive(Debug, Clone, Args)]
struct RuntimeArgs {
    /// Use this FFmpeg executable instead of Mimic's normal discovery order.
    #[arg(long, value_name = "PATH")]
    ffmpeg: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
struct FrameArgs {
    /// Number of frames required for the proof.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=10_000))]
    frames: u32,

    /// RGB output width used by the probe.
    #[arg(long, default_value_t = 320, value_parser = clap::value_parser!(u32).range(1..=7680))]
    width: u32,

    /// RGB output height used by the probe.
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u32).range(1..=4320))]
    height: u32,

    /// Requested capture/decode rate, from 1 through 120.
    #[arg(long, default_value_t = 30.0)]
    fps: f32,

    /// Hard timeout in seconds.
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(1..=7200))]
    timeout: u64,
}

#[derive(Debug, Args)]
struct MediaArgs {
    #[command(flatten)]
    runtime: RuntimeArgs,

    /// Media file to decode.
    #[arg(long, value_name = "PATH")]
    input: PathBuf,

    #[command(flatten)]
    frame: FrameArgs,
}

#[derive(Debug, Args)]
struct CameraArgs {
    #[command(flatten)]
    runtime: RuntimeArgs,

    /// Exact DirectShow device name reported by `mimic-doctor check`.
    #[arg(long, value_name = "NAME")]
    device: String,

    #[command(flatten)]
    frame: FrameArgs,
}

#[derive(Debug, Args)]
struct VirtualOutputArgs {
    #[command(flatten)]
    runtime: RuntimeArgs,

    #[command(flatten)]
    frame: FrameArgs,
}

#[derive(Debug, Args)]
struct SoakArgs {
    #[command(flatten)]
    runtime: RuntimeArgs,

    /// Media file to loop for the bounded soak.
    #[arg(long, value_name = "PATH")]
    input: PathBuf,

    /// Bounded soak duration in seconds (maximum one hour).
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=3600))]
    seconds: u64,

    /// RGB decode width.
    #[arg(long, default_value_t = 320, value_parser = clap::value_parser!(u32).range(1..=7680))]
    width: u32,

    /// RGB decode height.
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u32).range(1..=4320))]
    height: u32,

    /// Requested decode rate, from 1 through 120.
    #[arg(long, default_value_t = 30.0)]
    fps: f32,
}

fn main() {
    let cli = Cli::parse();
    let report = match cli.command {
        DoctorCommand::Check(args) => run_check(args.ffmpeg),
        DoctorCommand::Media(args) => run_media_probe(&MediaProbeOptions {
            ffmpeg: args.runtime.ffmpeg,
            input: args.input,
            frames: args.frame.frames,
            width: args.frame.width,
            height: args.frame.height,
            fps: args.frame.fps,
            timeout: Duration::from_secs(args.frame.timeout),
        }),
        DoctorCommand::Camera(args) => run_camera_probe(&CameraProbeOptions {
            ffmpeg: args.runtime.ffmpeg,
            device: args.device,
            frames: args.frame.frames,
            width: args.frame.width,
            height: args.frame.height,
            fps: args.frame.fps,
            timeout: Duration::from_secs(args.frame.timeout),
        }),
        DoctorCommand::VirtualOutput(args) => {
            run_virtual_output_probe(&VirtualOutputProbeOptions {
                ffmpeg: args.runtime.ffmpeg,
                frames: args.frame.frames,
                width: args.frame.width,
                height: args.frame.height,
                fps: args.frame.fps,
                timeout: Duration::from_secs(args.frame.timeout),
            })
        }
        DoctorCommand::Soak(args) => run_soak(&SoakOptions {
            ffmpeg: args.runtime.ffmpeg,
            input: args.input,
            duration: Duration::from_secs(args.seconds),
            width: args.width,
            height: args.height,
            fps: args.fps,
        }),
    };

    emit(&report, cli.json);
    std::process::exit(report.exit_code());
}

fn emit(report: &DoctorReport, json: bool) {
    let output = if json {
        serde_json::to_string_pretty(report).expect("doctor reports must serialize") + "\n"
    } else {
        report.render_text()
    };
    if let Err(error) = io::stdout().lock().write_all(output.as_bytes())
        && error.kind() != io::ErrorKind::BrokenPipe
    {
        let _ = writeln!(
            io::stderr().lock(),
            "mimic-doctor: could not write output: {error}"
        );
        std::process::exit(1);
    }
}
