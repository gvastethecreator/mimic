# Development and verification

Mimic is a Windows/MSVC application. Use focused checks while editing and run the full gate once at a meaningful checkpoint.

## Prerequisites

- Rust stable, MSVC x64 target.
- Visual Studio 2022 Build Tools with Desktop development with C++ and a Windows SDK.
- FFmpeg for runtime tests.
- Optional: OBS Virtual Camera or UnityCapture for output tests.

If `cargo` reports missing MSVC headers such as `vcruntime.h`, run it after `VsDevCmd.bat` instead of changing the crate:

```powershell
$devcmd = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat'
cmd /c "call `"$devcmd`" -no_logo -arch=x64 -host_arch=x64 && cargo check"
```

## Focused checks

```powershell
cargo test config::tests
cargo test decoder::tests
cargo test webcam::tests
cargo test compositor::tests
cargo test setup::tests
```

Cargo accepts one test-name filter at a time. Run separate commands when checking multiple modules.

## Checkpoint gate

```powershell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build
git diff --check
```

The all-platform lockfile currently contains `quick-xml 0.39.4` through Wayland-only scanner tooling even though it is absent from the Windows dependency graph. Until the planned egui/eframe migration removes it, make the target exception explicit instead of hiding it:

```powershell
cargo tree --target x86_64-pc-windows-msvc -i quick-xml
cargo audit --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195
```

The first command must remain empty. The second still prints maintenance and unsoundness warnings that should be reviewed during dependency migration.

Run Cargo commands through the x64 Visual Studio developer environment when the current shell does not already have `INCLUDE`, `LIB`, and the Windows SDK variables.

## Manual smoke test

Use disposable media and avoid committing user-specific settings or test files.

1. Start with no `%APPDATA%\mimic\config.json`; confirm the empty preview, setup status, and disabled Start reason fit at the minimum window size.
2. Add a short video through the file picker and by drag-and-drop. Confirm duplicates and unsupported files produce a summary.
3. Verify source dimensions/FPS, timeline movement, pause/play, seeking, end-of-item advance, and loop behavior.
4. Change resolution and FPS while stopped. Confirm the preview/decoder restart coherently and the selected values persist after relaunch.
5. Start output with a supported backend. Confirm the backend name, live state, disabled output controls, and Stop action. In a separate receiving application, select the same virtual camera and inspect the feed.
6. Stop output before closing Mimic. Confirm no Mimic-owned FFmpeg process remains.
7. For PiP, explicitly select a test camera, then verify each position, minimum/maximum size, rounded corners, device changes, capture failure feedback, and persistence. Never enable an unrelated user's camera during automated verification.
8. On a clean Windows VM, exercise FFmpeg download cancellation/failure/success, UnityCapture UAC denial/approval, restart detection, and uninstall/reinstall recovery.

A deterministic two-second fixture can be generated outside tracked source:

```powershell
ffmpeg -f lavfi -i testsrc2=size=640x360:rate=30 -t 2 -pix_fmt yuv420p mimic-sample.mp4
```

## Release evidence still required

Before calling a packaged release ready, retain evidence for:

- clean-machine build and launch;
- installer/uninstaller behavior and signatures;
- UnityCapture setup under UAC approval and denial;
- OBS and Unity receiver-side video at every exposed output format;
- physical-camera PiP on representative devices;
- long-running playback, playlist rollover, and resource usage;
- accessibility and keyboard traversal at the minimum window size.
