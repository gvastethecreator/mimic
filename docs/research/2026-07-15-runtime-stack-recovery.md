# Runtime stack recovery decision

Date: 2026-07-15

## Question

Should Mimic replace or upgrade its current `eframe`/`virtualcam` stack during the health-recovery slice, and which upstream constraints must the implementation respect?

## Answer

Keep the locked stack for this slice. The highest-risk failures are in Mimic's integration code, not evidence of an upstream API that requires migration. Repair backend detection, pin and validate setup downloads, propagate send failures, and update the existing egui texture instead of allocating one in every UI pass.

## Primary sources

- egui `TextureHandle`: <https://docs.rs/egui/0.27.2/egui/struct.TextureHandle.html>
- egui `Context::load_texture` documentation: <https://docs.rs/egui/0.27.2/egui/struct.Context.html#method.load_texture>
- `virtualcam` 0.1.1 package/source: <https://crates.io/crates/virtualcam/0.1.1> and <https://github.com/NeuroDonu/virtualcam>
- UnityCapture official README: <https://github.com/schellingb/UnityCapture>
- UnityCapture pinned source commit: <https://github.com/schellingb/UnityCapture/commit/3ed54c325e0ad71afcf4f246c07e5e17b3d7f2d2>
- `ffmpeg-static` official release API: <https://api.github.com/repos/eugeneware/ffmpeg-static/releases/latest>
- Repository-owned dependency lock and integration: `Cargo.lock`, `src/setup.rs`, `src/compositor.rs`, `src/gui.rs`.

## Findings

1. `TextureHandle::set` replaces the image behind an existing texture. egui's `load_texture` docs explicitly say to allocate once rather than from main GUI code. Mimic currently calls `load_texture` on every repaint, so the safe local fix is to retain one handle and call `set` for new preview frames.
2. `virtualcam` 0.1.1 supports OBS Virtual Camera and Unity Video Capture on Windows and converts RGB input to the backend's native format. Mimic should consider either verified backend ready instead of gating all streaming on a Unity-only registry check.
3. The `virtualcam` 0.1.1 source checks Unity devices under CLSIDs beginning `5C2CD55C-92AD-4999-8666-912BD3E700`. Mimic checks unrelated CLSID `A91FD3C7-15E8-4e89-940E-6F3C01234567`; this can report the driver missing when the supported backend is installed.
4. UnityCapture's official instructions require registration from a stable install location and warn that the filter must be uninstalled before its files are moved or deleted. Mimic therefore needs a stable AppData path and must verify registration after elevation instead of treating process launch as success.
5. The configured FFmpeg URL, `.../b5.0.1/win32-x64`, does not match a release asset. As of 2026-07-15, the official GitHub release API reports tag `b6.1.1`, asset `ffmpeg-win32-x64`, 82,797,568 bytes, and SHA-256 `04e1307997530f9cf2fe35cba2ca7e8875ca91da02f89d6c7243df819c94ad00`.
6. UnityCapture has no GitHub Releases artifact. Pinning its official `Install/UnityCaptureFilter64.dll` to commit `3ed54c325e0ad71afcf4f246c07e5e17b3d7f2d2` avoids mutable-master downloads; the fetched file is 157,696 bytes with SHA-256 `72812f5363d8ecb45632253f8c8c888844b1b62e27616f3c8cc21064ccde25e5`.

## Uncertainty and later runtime evidence

- A later native smoke test on this machine detected both supported registry backends. Starting output selected OBS Virtual Camera, reported the live backend, accepted composited frames, and stopped cleanly. A separate receiving application was not used, so receiver-side video remains a release gate.
- UnityCapture download and elevated registration were intentionally not exercised on this configured machine because they modify system state. That path still needs clean-VM proof including UAC denial and approval.
- The upstream `virtualcam` backend-selection behavior is retained. Mimic surfaces the selected backend and send errors rather than introducing a custom camera transport.
- Dependency upgrades may still be worthwhile later, but they should be a separate migration with API, visual, and performance baselines.

## Decision impact

- No dependency upgrade in the recovery batch.
- Add backend-specific readiness detection and accept OBS or Unity.
- Make downloads pinned, atomic, hash-verified, and executable/registration-verified.
- Reuse the preview texture and repaint at the configured frame cadence.
- Treat backend initialization and receiver-side rendering as separate gates; successful frame submission does not by itself prove a receiving application's display path.
