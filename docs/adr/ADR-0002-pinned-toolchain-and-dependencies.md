# ADR-0002: Pin toolchain and modernize dependencies deliberately

- Status: Accepted
- Date: 2026-07-15

## Context

The alpha uses eframe 0.27 and Rust 1.90 locally. Current eframe requires Rust 1.92 and
contains breaking application lifecycle changes. The old graph also produces audit
warnings and target-inapplicable Wayland advisories in the all-platform lockfile.

## Decision

Pin the repository to Rust 1.92 with rustfmt and clippy. Upgrade direct dependencies in
reviewable slices, starting with the UI/runtime stack, and keep `virtualcam 0.1.1`
because no newer release exists. Each migration must pass focused tests, Windows target
graph inspection, clippy, release build, and native UI/output smoke before acceptance.

Audit policy is target-aware and explicit. Advisory exceptions need a checked-in reason,
expiry/review instruction, and evidence that the affected package is absent from the
Windows runtime graph. Suppression without that evidence is not accepted.

## Consequences

- Contributors and CI use the same supported compiler.
- Compiler download is a one-time setup cost.
- Dependency modernization cannot be declared complete from `cargo update` alone.
