# Changelog

All notable changes to Mimic are recorded here. The project has not published a stable release yet.

## Unreleased

### Added

- OBS Virtual Camera readiness alongside Unity Video Capture.
- Explicit setup, empty, loading, playing, paused, live, success, and failure feedback.
- Playlist removal, duplicate/unsupported filtering, missing-file state, automatic advance, and looping.
- Source metadata, seek timeline, output status, and persistent PiP controls.
- Atomic settings persistence with normalization and corrupt-file recovery.
- Pinned size- and SHA-256-verified FFmpeg and UnityCapture downloads.
- Unit coverage for configuration, metadata parsing, webcam parsing, composition boundaries, backend labels, and integrity helpers.
- Runtime architecture, development verification, and upstream research documentation.
- `mimic-doctor` environment, media, physical-camera, virtual-receiver, and bounded-soak
  probes with human/JSON output and stable exit codes.
- Bounded rotating local diagnostics with no frame retention or remote telemetry.
- Windows CI, target-aware dependency audit policy, deterministic portable packaging,
  provenance/checksum verification, and optional fail-closed Authenticode signing.
- Automatic MSVC x64/Windows SDK environment discovery for release/package scripts.
- A build-derived multi-resolution Windows application icon and version metadata.
- ADR, task, research, runbook, and release-evidence ledgers.

### Changed

- Reworked the native layout into a compact studio surface with actionable setup and start blockers.
- Reused a single egui preview texture and scheduled repainting at the output cadence.
- Replaced unbounded frame delivery with bounded channels to protect responsiveness.
- Moved playlist/configuration rules out of the UI into a tested domain module.
- Locked output format while the virtual camera is active.
- Disabled unused eframe Wayland/X11/web defaults in this Windows-only application while retaining accessibility, fonts, and the Glow renderer.
- Pinned Rust 1.92 and migrated eframe to 0.35, rfd to 0.17, ureq to 3.3, sha2
  to 0.11, and windows-sys to 0.61.
- Split reusable runtime modules into a library consumed by the GUI and diagnostics.

### Fixed

- Corrected the Unity device detection path and stopped treating OBS installations as unavailable.
- Propagated decoder, webcam, virtual-camera initialization, and frame-send failures instead of silently continuing.
- Ensured FFmpeg children and virtual-camera handles are released when replaced, stopped, or closed.
- Normalized malformed RGB frame lengths and clamped PiP composition to output bounds.
- Replaced invalid mutable setup URLs with pinned upstream artifacts.
- Prevented repeated preview-texture allocation and continuous busy repainting.
- Parsed modern headerless FFmpeg DirectShow device listings instead of silently
  reporting no cameras.
- Warmed virtual output before independent receiver capture, eliminating startup-frame
  loss from the verification path.
- Added explicit accessible names to resolution, frame-rate, physical-source,
  placement, and playback-position controls.
