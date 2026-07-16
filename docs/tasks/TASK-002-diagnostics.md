# TASK-002: Scriptable diagnostics and verification seams

- Status: done
- ADRs: [ADR-0003](../adr/ADR-0003-scriptable-diagnostics.md),
  [ADR-0004](../adr/ADR-0004-evidence-and-privacy-boundaries.md)

## Checklist

- [x] Implement `mimic-doctor` command parsing, help, version, and exit codes.
- [x] Provide human and JSON outputs without mixed stdout noise.
- [x] Implement environment/FFmpeg/backend checks.
- [x] Implement bounded media decoding with timeout and cleanup.
- [x] Implement explicit physical-camera frame counting without image retention.
- [x] Implement deterministic virtual-output sender plus FFmpeg receiver proof.
- [x] Implement bounded soak metrics and process cleanup checks.
- [x] Add deterministic fixtures and unit/integration coverage.

## Evidence

Focused doctor/diagnostic/webcam tests passed. Real probes decoded five fixture frames,
counted three authorized camera frames without retained imagery, delivered ten frames to
an independent OBS/FFmpeg receiver, and completed a five-second resource soak. Exact
sanitized measurements are in the [evidence report](../release-readiness-2026-07-15.md).
