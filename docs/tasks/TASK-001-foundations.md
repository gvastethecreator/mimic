# TASK-001: Toolchain and application foundations

- Status: done
- ADRs: [ADR-0002](../adr/ADR-0002-pinned-toolchain-and-dependencies.md)

## Checklist

- [x] Pin Rust 1.92 with rustfmt and clippy.
- [x] Add package description, license, repository, keywords, categories, and MSRV.
- [x] Add an intentional release profile.
- [x] Upgrade eframe/rfd/ureq and reconcile Windows APIs.
- [x] Inspect Windows dependency graph and audit findings.
- [x] Add a recognizable Windows icon/application identity.
- [x] Extract reusable library seams for GUI and diagnostics.
- [x] Pass focused tests, clippy, and release build.

## Evidence

`cargo check --all-targets`, focused module tests, `cargo clippy --all-targets --
-D warnings`, and release builds passed under Rust 1.92. The Windows icon was visually
inspected from the generated 256 px resource. Target-aware audit evidence is enforced by
`scripts/verify-audit.ps1`.
