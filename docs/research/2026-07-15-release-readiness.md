# Release-readiness research — 2026-07-15

## Question

What can Mimic honestly implement and verify in this repository to move from an alpha
runtime to a Windows release candidate, and which claims still require external signing
or clean-machine infrastructure?

## Current evidence

- Local compiler: Rust 1.90, `x86_64-pc-windows-msvc`.
- Current manifest: `eframe 0.27`, `rfd 0.14`, `ureq 2.9`, `virtualcam 0.1.1`.
- Current published candidates: `eframe 0.35.0` (MSRV 1.92), `rfd 0.17.2`,
  `ureq 3.3.0`; `virtualcam` remains at 0.1.1.
- Local release tools: Cargo, rustup, FFmpeg, GitHub CLI, and cargo-audit are present.
  SignTool, WiX, Inno Setup, MakeAppx, and an application signing identity are absent.
- DirectShow discovery sees an authorized physical-device candidate (`Logitech BRIO`)
  and the Unity virtual device. The prior recovery report proved sender-side OBS output,
  but no receiver was attached.

## Primary sources

- GitHub, *Building and testing Rust*: <https://docs.github.com/en/actions/tutorials/build-and-test-code/rust>
- Cargo, *Profiles*: <https://doc.rust-lang.org/cargo/reference/profiles.html>
- rustup, *Overrides*: <https://rust-lang.github.io/rustup/overrides.html>
- eframe changelog: <https://github.com/emilk/egui/blob/main/crates/eframe/CHANGELOG.md>
- eframe package: <https://crates.io/crates/eframe/0.35.0>
- rfd package: <https://crates.io/crates/rfd/0.17.2>
- ureq package: <https://crates.io/crates/ureq/3.3.0>
- virtualcam package: <https://crates.io/crates/virtualcam/0.1.1>
- Microsoft, *SignTool*: <https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool>
- Microsoft, *Sign an app package using SignTool*:
  <https://learn.microsoft.com/en-us/windows/msix/package/sign-app-package-using-signtool>

## Findings

1. Modernizing eframe is not a lockfile-only update. The current release raises the
   compiler floor and changes application APIs, so compiler pinning, source migration,
   visual checks, and native runtime checks must move together.
2. GitHub's Windows runners can build Rust projects, but an explicit rustup toolchain
   step is needed to reproduce the local compiler contract instead of relying on runner
   drift.
3. Authenticode and MSIX signing require a certificate whose subject matches the package
   identity. A repository cannot manufacture that trust. It can provide deterministic
   unsigned artifacts, an optional SignTool hook, and verification instructions.
4. A portable ZIP is the strongest release artifact this machine can prove today. An
   MSI/MSIX decision should wait for clean-VM driver lifecycle evidence because Mimic can
   install/register a virtual-camera backend with elevation.
5. Release readiness needs receiver-side evidence. A successful `send_frame` call is an
   application-side assertion; a bounded FFmpeg DirectShow capture supplies an
   independent receiving process and deterministic frame/hash evidence.
6. Diagnostics should be a separate console binary. Human-readable output, JSON, stable
   exit codes, timeouts, and cleanup make environment and media failures reproducible in
   support and CI without compromising the GUI subsystem.

## Implementation consequences

- Pin Rust 1.92 and migrate direct dependencies one measured slice at a time.
- Keep `virtualcam` pinned to 0.1.1 and isolate it behind application-owned probes.
- Ship `mimic.exe` plus `mimic-doctor.exe` in a checksummed portable archive.
- Make signing optional and fail closed when requested without SignTool/certificate.
- Keep clean-machine installer/signature/UAC proof as an explicit external release gate.
