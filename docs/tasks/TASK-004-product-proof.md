# TASK-004: Product-path quality proof

- Status: done
- ADRs: [ADR-0004](../adr/ADR-0004-evidence-and-privacy-boundaries.md)

## Checklist

- [x] Verify/fix keyboard traversal, focus visibility, and accessible names.
- [x] Verify/fix constrained-window copy/layout and error/recovery states.
- [x] Exercise real video playlist playback and persistence.
- [x] Exercise authorized physical-camera frame flow without retaining imagery.
- [x] Prove virtual-camera output in an independent receiver.
- [x] Run bounded soak and record timing/resource/process cleanup.
- [x] Retain sanitized textual/JSON evidence where appropriate.
- [x] Reconcile user and maintainer documentation with actual proof.

## Evidence

Native inspection covered empty, PiP warning, picker, loaded playback, focus traversal,
and constrained layout states. It found and fixed unnamed combo/slider controls. Runtime
proofs are recorded without private media paths or camera images in the
[evidence report](../release-readiness-2026-07-15.md).
