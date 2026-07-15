# Health recovery report - 2026-07-15

Baseline: commit `80e9297` on `master`.

This recovery slice moved Mimic from a raw, Unity-only happy path to a tested Windows application flow with explicit setup, playlist, playback, preview, output, persistence, and recovery states. The mission covered source, native runtime behavior, documentation, and proof; it did not attempt release packaging or system-level installation on a configured workstation.

## Before and after

| Area | Baseline | Recovery result |
| --- | --- | --- |
| Setup | Invalid mutable FFmpeg URL; unrelated Unity CLSID; launch of registration treated as success | Pinned URL/size/SHA-256, atomic activation, executable/registry verification, OBS or Unity readiness, retry action and bounded download timeout |
| Configuration | UI-owned values with weak validation/write feedback | Tested schema, normalization, playlist rules, atomic replacement, corrupt-file warning |
| Media | Unbounded/silent worker behavior and limited lifecycle feedback | Bounded frame delivery, explicit end/failure events, child cleanup, metadata/timeline, still-image looping, playlist advance/loop |
| Webcam | Errors and child lifetime were easy to lose | Bounded frames, device parser, visible capture failures, cleanup, persistent PiP controls |
| Composition/output | Send failures ignored; frame assumptions unchecked | Frame normalization, bounded rounded PiP, surfaced send errors, explicit backend/device and live/stop lifecycle |
| Preview/UI | Texture allocated repeatedly, continuous repaint, incomplete states and clipped controls | Reused texture, cadence-based repaint, responsive three-column studio, actionable empty/setup/playing/paused/live/error states |
| Documentation | README described a different FFmpeg-to-Unity pipeline | README, architecture, development, research, changelog, limits and this evidence report aligned to source |

## Evidence loops

1. Baseline source/runtime and MSVC-environment isolation.
2. Pinned setup artifacts and integrity helpers.
3. OBS/Unity backend discovery and registration semantics.
4. Configuration normalization and atomic persistence.
5. Playlist add/remove/dedupe/advance/loop rules.
6. Decoder metadata, bounded events, end/error, seek and child lifetime.
7. Webcam parsing, bounded capture, failure and child lifetime.
8. Frame normalization, PiP bounds and output-error propagation.
9. Preview texture reuse, dirty tracking and repaint cadence.
10. Native hierarchy and empty/playing layout review. Verdict: continue because live output, docs and final gates were still missing.
11. Generated-media playback, metadata, timeline and rollover.
12. Application-side OBS virtual-camera start, live-frame submission and stop.
13. Still-image loop command and final clean-start visual inspection.
14. Documentation-to-source reconciliation and dependency advisory audit.

## Verification manifest

| Gate | Result | Evidence |
| --- | --- | --- |
| Format | Pass | `cargo fmt --check` |
| Unit tests | Pass | `cargo test --all-targets`: 19 passed, 0 failed |
| Lint | Pass | `cargo clippy --all-targets -- -D warnings` |
| Build | Pass | `cargo build` in the VS 2022 x64 developer environment |
| Patch whitespace | Pass | `git diff --check` |
| Video playback | Pass | Generated 640 x 360, 30 FPS, two-second fixture decoded, previewed, reached the end and looped |
| Still-image command | Pass | PNG input with Mimic's loop/framerate/filter path produced six frames over 0.20 seconds |
| Virtual output | Pass, application side | OBS backend selected, live state shown, frames accepted, Stop used before exit |
| Final native UI | Pass | Exact final debug build inspected at 1120 x 760: empty state, blocker, full-width Add action, format controls and transport fit without scrolling |
| User-state cleanup | Pass | Mimic process stopped; temporary `%APPDATA%\mimic\config.json` absent after verification |

Cargo commands require the matching MSVC environment on this host. The ambient shell initially failed in native dependencies because `INCLUDE` did not contain `vcruntime.h`; no product dependency change was used to hide that host issue.

## Advisory audit

Raw `cargo audit` reports two high-severity `quick-xml 0.39.4` advisories ([RUSTSEC-2026-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194) and [RUSTSEC-2026-0195](https://rustsec.org/advisories/RUSTSEC-2026-0195)). `cargo tree --target x86_64-pc-windows-msvc -i quick-xml` is empty: the crate enters the all-platform lockfile through Wayland scanner tooling and is not in Mimic's Windows build graph. The audit passes when those two target-inapplicable findings are explicitly ignored, while still reporting six allowed maintenance/unsoundness warnings from the older egui ecosystem.

Mimic now disables eframe's unused default Wayland/X11/web features and enables only `accesskit`, default fonts, and `glow`. This reduced the resolved lockfile, but an egui/eframe migration is still the correct follow-up for a globally clean advisory report; forcing an incompatible `quick-xml` patch into an old Wayland build dependency is not a safe fix.

## Adversarial close

Three internal lenses were used because independent-agent delegation was not authorized for this task:

- Realtime media engineer: strongest objection is that accepting frames in `virtualcam` does not prove a receiver renders them. Result: application-side path passes; receiver-side OBS/Unity display remains a release gate.
- Native product reviewer: strongest objection is incomplete input and viewport coverage. Result: empty/playing/live states and accessibility text were inspected, but native picker selection automation failed after three attempts and full keyboard traversal remains open.
- Security/privacy reviewer: strongest objection is system mutation and camera access. Result: downloads are pinned/verified; Unity UAC registration and physical-camera capture were deliberately not exercised without a clean VM or explicit hardware authorization.

Final axes:

- Task state: completed for the requested health-recovery slice.
- Artifact verdict: clear improvement over baseline and usable for continued development.
- Verification state: limited, not release-ready, because receiver-side video, Unity installation, physical-camera hardware, long soak, and packaging are not yet proved.

## Recommended next slice

Build a repeatable clean-Windows release harness: packaged binary, installer/uninstaller, UAC denial/approval, OBS and Unity receiver captures at every exposed format, physical-camera PiP on authorized hardware, and a long-running playlist/resource soak. Plan the egui/eframe dependency migration as a separate measured change with visual and runtime baselines.
