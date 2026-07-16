# Windows release runbook

This runbook produces and verifies an **unsigned release-candidate artifact**. It does
not turn an unverified build into a signed public release; see
[ADR-0001](../adr/ADR-0001-portable-windows-release.md) and
[TASK-005](../tasks/TASK-005-external-release-gates.md).

## Prerequisites

- Windows x64 with Visual Studio C++ Build Tools.
- The pinned Rust toolchain from `rust-toolchain.toml`.
- `cargo-audit 0.22.2` for the full gate:
  `cargo install cargo-audit --version 0.22.2 --locked`.
- FFmpeg is optional for compilation and package verification; `mimic-doctor check`
  reports it as unavailable rather than mutating the machine.

## Full local gate

```powershell
./scripts/release-gate.ps1
```

The gate runs formatting, all tests, clippy with warnings denied, the target-aware
advisory policy, release builds, deterministic packaging, manifest verification, and a
packaged `mimic-doctor` smoke.

## Package only

```powershell
./scripts/package.ps1
./scripts/verify-package.ps1 -Archive ./dist/mimic-v0.1.0-windows-x64.zip
```

The ZIP contains:

- `mimic.exe` — GUI application;
- `mimic-doctor.exe` — console diagnostics and bounded proof commands;
- README, changelog, license, release runbook, third-party dependency inventory;
- `provenance.json` — commit, toolchain, lockfile hash, target, and signing state;
- `manifest.sha256` — checksum for every packaged file except the manifest itself.

The archive uses sorted entries and the commit timestamp (`SOURCE_DATE_EPOCH` semantics)
so identical inputs produce stable package structure. A sidecar SHA-256 covers the ZIP.

## Optional Authenticode signing

Signing is fail-closed and requires a certificate already installed in the Windows
certificate store plus SignTool from the Windows SDK:

```powershell
./scripts/package.ps1 `
  -Sign `
  -CertificateThumbprint '<certificate thumbprint>' `
  -TimestampUrl 'http://timestamp.digicert.com'
```

Both executables are signed with SHA-256, RFC 3161 timestamped, and verified before the
manifest and ZIP are created. Never place a PFX password or private key in source,
command history, or artifact provenance.

## Install and remove the portable candidate

1. Verify the ZIP and sidecar with `verify-package.ps1`.
2. Extract the single root directory to a user-owned location.
3. Run `mimic-doctor check`, then launch `mimic.exe`.
4. Before removal, stop virtual output and close Mimic.
5. Remove the extracted directory. Mimic's settings/logs remain under `%APPDATA%\mimic`;
   delete that directory only when the user explicitly wants to reset local state.

Removing the portable directory does **not** uninstall OBS Virtual Camera or Unity
Capture. Driver lifecycle belongs to the external clean-machine gate.

## Diagnostic proofs

```powershell
mimic-doctor check --json
mimic-doctor media --input ./sample.mp4 --frames 5 --json
mimic-doctor camera --device 'Exact DirectShow name' --frames 3 --json
mimic-doctor virtual-output --frames 10 --json
mimic-doctor soak --input ./sample.mp4 --seconds 300 --json
```

`camera` is explicit and retains no image. `virtual-output` warms the sender before
starting FFmpeg, then requires receiver-side frame hashes. All proof commands are
bounded and return stable exit codes: `0` pass, `2` invalid input, `3` unavailable
dependency/device, `4` failed/timed-out proof.

## Audit policy

Raw `cargo audit` sees `quick-xml 0.39.4` through Linux Wayland build tooling and reports
RUSTSEC-2026-0194/0195. `verify-audit.ps1` first fails if `quick-xml`, `anyhow`, or
`memmap2` enters `x86_64-pc-windows-msvc`, then ignores only those two Wayland
vulnerability IDs when running the full lockfile audit. A dependency change that brings
them into the Windows graph therefore fails before the ignore is applied.

## External distribution gate

Before a public stable release, provide a real signing identity and clean Windows VM,
then prove UAC denial/approval, backend installation, repair, upgrade, uninstall,
rollback, and both OBS/Unity receiver paths. Do not rename the unsigned CI artifact to
imply those claims.
