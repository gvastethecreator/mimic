# Development and verification

Mimic is a Windows/MSVC application. Use focused checks while editing and run the full gate once at a meaningful checkpoint.

## Prerequisites

- Rust 1.92 as pinned by `rust-toolchain.toml`, with the MSVC x64 target, rustfmt, and
  clippy. Let rustup honor the repository override.
- Visual Studio 2022 Build Tools with Desktop development with C++ and a Windows SDK.
- FFmpeg for runtime tests.
- Optional: OBS Virtual Camera or UnityCapture for output tests.

The release/package scripts discover Visual Studio with `vswhere` and import a coherent
x64 developer environment automatically. For ad-hoc Cargo commands, if `cargo` reports
missing MSVC headers such as `vcruntime.h`, initialize the shell first:

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
cargo test doctor::tests
cargo test diagnostics::tests
```

Cargo accepts one test-name filter at a time. Run separate commands when checking multiple modules.

## Checkpoint gate

```powershell
./scripts/release-gate.ps1
```

The script runs format, all-target tests, clippy with warnings denied, target-aware audit,
release builds, packaging, and extracted-package verification. During a focused audit,
run:

```powershell
./scripts/verify-audit.ps1
```

The all-platform lockfile contains vulnerable `quick-xml 0.39.4` only through
Wayland build tooling. The audit script first proves `quick-xml`, `anyhow`, and `memmap2`
are absent from the Windows target graph, then applies the two documented Wayland-only
advisory exceptions. A future dependency edge into the Windows product fails the gate.

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

## Release evidence obtained

The dated [release-readiness report](release-readiness-2026-07-15.md) records:

- focused and full automated checks;
- real media decode and bounded soak metrics;
- explicit physical-camera frame proof without retained images;
- OBS virtual-camera frames captured by an independent FFmpeg receiver;
- native layout, keyboard focus, accessibility-name, and file-picker inspection;
- repeatable package generation and extracted-package smoke.

## External release evidence still required

Before calling a packaged release ready, retain evidence for:

- clean-machine launch and trust behavior;
- installer/repair/upgrade/uninstaller behavior and signatures;
- UnityCapture setup under UAC approval and denial;
- OBS and Unity receiver-side video at every exposed output format on the release image;
- longer multi-hour endurance and broader hardware coverage.
