# Release-readiness evidence — 2026-07-15

## Verdict

Repository-addressable work for the unsigned Windows `0.1.0` release candidate is
implemented and locally proved. This is not evidence for a signed public release:
[TASK-005](tasks/TASK-005-external-release-gates.md) remains `blocked_external` pending
a signing identity, clean Windows VM, and authorized driver/UAC lifecycle testing.

## Automated evidence

| Surface | Evidence | Result |
|---|---|---|
| Compiler/dependencies | Rust 1.92; all targets checked; migrated direct dependencies | pass |
| Focused regressions | doctor (4), webcam (4), diagnostics (2), including headerless DirectShow parsing | pass |
| Static quality | rustfmt; clippy all targets with warnings denied | pass |
| Full tests | `cargo test --locked --all-targets` | pass |
| Audit | Windows graph excludes the documented Wayland-only vulnerable/transitive crates | pass |
| Release binaries | `mimic.exe` and `mimic-doctor.exe` under the release profile | pass |
| Packaging | deterministic ZIP, file manifest, sidecar SHA-256, provenance | pass |
| Extracted smoke | manifest/provenance, doctor version, and doctor environment check | pass |
| Normal-shell bootstrap | `vswhere` selects one coherent MSVC x64 and Windows SDK environment | pass |
| Final local gate | `scripts/release-gate.ps1`; 26 tests; release/package/smoke | pass |

The release scripts are also the Windows CI contract in `.github/workflows/ci.yml`.
PowerShell parsed all five scripts without syntax errors before the final gate.
The ZIP sidecar emitted by the final clean commit is the artifact checksum authority;
the hash is intentionally not embedded in source because package provenance includes
the commit that produced it.

The first final-gate attempt exposed a mixed Visual Studio environment with `LIB` set
but no matching `INCLUDE`; native dependency compilation failed on `vcruntime.h`. The
release/package scripts now discover Visual Studio and import one coherent x64 compiler
and SDK environment before Cargo runs. This was a release-operability defect, not waived
as a workstation quirk.

## Runtime proof

All probes used disposable or explicitly authorized inputs. No media path, decoded frame,
or camera image is retained in this report.

| Probe | Sanitized observation | Result |
|---|---|---|
| Environment | FFmpeg, OBS, Unity, and two DirectShow video devices detected | pass |
| Invalid media | Missing input produced typed JSON and exit code 2 | pass |
| Media | 5/5 frames at 640 x 360, 30 FPS, bounded aggregate hash | pass |
| Physical camera | 3/3 frames at 320 x 180; aggregate hash only; no payload retained | pass |
| Virtual output | Independent FFmpeg receiver captured 10/10 OBS frames after sender warmup | pass |
| Five-second soak | 119 frames (minimum 75), one loop, maximum frame gap 635 ms | pass |
| Soak memory | start 10,215,424 B; peak 10,919,936 B; end 10,911,744 B; growth 696,320 B | pass |
| Cleanup | owned decoder/receiver processes exited after probes | pass |

The first receiver attempt exposed a real startup race: the OBS backend was still
starting when the receiver attached. The diagnostic now sends three warmup frames before
receiver capture; the independent 10-frame proof then passed.

## Native product proof

The release GUI was inspected as a native Windows application at its minimum supported
layout. Empty/setup/playback/PiP-warning states remained readable and actionable. The
file picker opened as a native common dialog, and real playback was observed without
recording user media content or paths.

Accessibility-tree inspection found unnamed resolution, frame-rate, physical-source,
placement, and playback-position controls. Explicit names were added and observed in the
rebuilt application. Keyboard traversal reached format controls, PiP, media import, and
playlist-loop controls with visible focus. Native automation snapshots occasionally
lagged one key event, so this evidence supports operability but does not claim exhaustive
screen-reader certification.

## Quality loops and adversarial autopsy

| Loop | Risk attacked | Evidence/change |
|---:|---|---|
| 1 | Incomplete scope | ADR/task/plan cross-check mapped every known gate |
| 2 | Toolchain/dependency drift | Pinned Rust and target-aware audit |
| 3 | Ambiguous CLI | Typed clap contract, help/version, stable exits |
| 4 | Weak environment support | Human/JSON environment report |
| 5 | Probe leakage/cleanup | Bounded media/camera work and cleanup checks |
| 6 | Sender-only false positive | Independent FFmpeg receiver; warmup race fixed |
| 7 | Non-repeatable distribution | Repeat package SHA and extracted verification |
| 8 | Local/CI divergence | CI invokes the same release-gate scripts |
| 9 | Inaccessible native controls | AccessKit inspection and explicit labels |
| 10 | Resource instability | Timed soak, progress, gaps, memory, cleanup |

Adversarial lenses reached the same bounded conclusion:

- Product: core states, feedback, focus, accessibility names, and real playback are
  usable; broader assistive-technology certification is not claimed.
- Realtime/runtime: receiver-side output and cleanup are proved; a five-second soak is a
  regression gate, not a multi-hour endurance claim.
- Release/security: dependency exceptions are target-enforced and packaging is
  reproducible; unsigned code is never presented as trusted/signed.
- Operations/privacy: support output is structured and bounded; camera/media payloads
  and private paths are excluded from retained evidence.

No independent agent review was performed because delegation was not authorized for
this task. The final internal quality verdict is **stop**: no further repository-only
change is justified by current evidence; continue through TASK-005 only when its external
inputs are available.
