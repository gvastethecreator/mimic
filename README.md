# Mimic

Mimic is a Windows virtual-camera studio written in Rust. It plays a local media playlist, can place a physical webcam over the program feed, previews the result, and sends RGB frames to an installed OBS Virtual Camera or Unity Video Capture device.

Version `0.1.0` now has a reproducible **unsigned portable release-candidate**
workflow. Runtime, diagnostics, packaging, and receiver-side OBS output are proved on the
development workstation. A signed public release still requires the external gates in
[TASK-005](docs/tasks/TASK-005-external-release-gates.md).

## Current capabilities

- Playlist input for MP4, MKV, AVI, MOV, GIF, PNG, JPG, and JPEG files supported by FFmpeg.
- Drag-and-drop or native file-picker import, duplicate filtering, missing-file feedback, remove/select controls, automatic advance, and playlist looping.
- Play, pause, seek, elapsed/duration, source dimensions/FPS, and explicit loading/error/empty states.
- Output at 1280 x 720, 1920 x 1080, or 640 x 480; 30 or 60 FPS.
- Optional physical-webcam overlay with position, size, and corner-radius controls.
- OBS Virtual Camera and Unity Video Capture detection through the `virtualcam` backend.
- Pinned, SHA-256-verified fallback downloads for FFmpeg and UnityCapture.
- Atomic settings persistence with corrupt-file recovery.
- Scriptable environment, media, physical-camera, virtual-output, and soak diagnostics.
- Bounded local diagnostic logs with rotation and no video-frame retention.
- Reproducible portable ZIP packaging with manifests, provenance, and an optional
  fail-closed Authenticode signing hook.

Mimic does not send audio. It is Windows-only and needs at least one supported virtual-camera backend before output can start.

## Quick start from source

Requirements:

- Windows 10 or 11 (x64).
- The Rust toolchain declared in `rust-toolchain.toml` (rustup selects it automatically).
- Visual Studio 2022 Build Tools with Desktop development with C++.
- FFmpeg in `PATH`, or use Mimic's verified in-app download.
- OBS Virtual Camera or Unity Video Capture.

Run from a Visual Studio x64 Developer Command Prompt:

```powershell
cargo run --release --bin mimic
```

On machines where a regular shell cannot find the MSVC headers, initialize the build environment first:

```powershell
cmd /c '"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" -no_logo -arch=x64 -host_arch=x64 && cargo run --release --bin mimic'
```

## Using Mimic

1. Confirm that the setup banner reports FFmpeg and at least one virtual-camera backend ready.
2. Add or drop one or more media files. Mimic selects the first accepted item.
3. Choose output resolution and frame rate before starting output.
4. Optionally enable the physical-camera overlay and choose a device. Selecting a real camera opens that device through FFmpeg.
5. Select **Start virtual camera**. The UI reports the backend selected by `virtualcam` and locks output-format changes while live.
6. In the receiving application, choose the corresponding OBS or Unity virtual camera.

If a previously selected PiP device remains enabled in settings, Mimic resumes that camera on the next launch. Disable PiP before closing when automatic camera resume is not desired.

UnityCapture installation downloads a pinned DLL to `%APPDATA%\mimic` and asks Windows for administrator approval through `regsvr32`. Mimic checks that the device is actually registered before reporting success. OBS users can install OBS through its official distribution instead.

## Diagnose an installation

The portable package includes `mimic-doctor.exe`. It does not modify the machine and
uses stable exit codes (`0` pass, `2` invalid input, `3` unavailable dependency/device,
`4` failed or timed-out proof):

```powershell
mimic-doctor check
mimic-doctor media --input .\sample.mp4 --frames 5 --json
mimic-doctor virtual-output --frames 10 --json
mimic-doctor soak --input .\sample.mp4 --seconds 300 --json
```

Physical-camera probing is always explicit (`camera --device <exact name>`), bounded,
and retains hashes/counts rather than images. See the
[release runbook](docs/release/runbook.md) for the complete contract.

## Data and setup locations

| Item | Location |
| --- | --- |
| Settings | `%APPDATA%\mimic\config.json` |
| Downloaded FFmpeg fallback | `%APPDATA%\mimic\ffmpeg.exe` |
| Downloaded UnityCapture DLL | `%APPDATA%\mimic\UnityCaptureFilter64.dll` |
| Rotating diagnostic log | `%APPDATA%\mimic\logs\mimic.jsonl` |

Settings are written through a temporary file and atomically replaced. A malformed settings file is not overwritten silently: Mimic loads safe defaults and shows a warning.

## Architecture and development

- [Runtime architecture and failure model](docs/architecture.md)
- [Build, test, and manual verification guide](docs/development.md)
- [Architecture decision records](docs/adr/README.md)
- [Release-readiness task ledger](docs/tasks/README.md)
- [Windows release runbook](docs/release/runbook.md)
- [2026-07-15 release-readiness evidence](docs/release-readiness-2026-07-15.md)
- [Runtime-stack research decision](docs/research/2026-07-15-runtime-stack-recovery.md)
- [Release-readiness research](docs/research/2026-07-15-release-readiness.md)
- [2026-07-15 health-recovery evidence report](docs/health-recovery-2026-07-15.md)
- [Change log](CHANGELOG.md)

The short runtime flow is:

```text
media file -> FFmpeg RGB decoder ----\
                                      compositor -> egui preview
physical camera -> FFmpeg RGB capture/             -> virtualcam -> OBS or Unity device
```

## Known limits

- The produced ZIP is unsigned. Certificate-backed signing, clean-machine trust, and
  installer/driver lifecycle remain external release gates.
- UnityCapture UAC denial/approval and uninstall/reinstall recovery still need a clean
  release environment.
- A physical-camera feed is privacy-sensitive and requires an explicit device selection;
  the local proof records counts and hashes only.
- External receiving applications may impose their own format support and camera-locking rules.
- The app has no audio pipeline, scene transitions, packaged installer, or automatic update system.

## License

Mimic is licensed under the [MIT License](LICENSE).
