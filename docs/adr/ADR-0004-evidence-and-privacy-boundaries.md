# ADR-0004: Evidence and privacy boundaries

- Status: Accepted
- Date: 2026-07-15

## Context

Virtual-camera and physical-camera tests can overclaim success or accidentally retain
sensitive imagery. Sender acceptance does not prove a receiving application rendered the
stream.

## Decision

Use separate evidence gates:

1. Sender gate: backend initializes, accepts deterministic frames, and releases cleanly.
2. Receiver gate: an independent receiving process observes bounded frames from the
   named virtual device and reports a deterministic hash/frame count.
3. Product gate: native GUI flows recover from setup, media, camera, and output failures.

Physical-camera evidence records only device name, negotiated dimensions, frame count,
timing, and optional aggregate byte hash. It must not save screenshots or frame payloads.
The command is never run implicitly by `check`, CI, or packaging.

Logs redact user paths where practical, never include downloaded content or frame bytes,
and have bounded retention. No telemetry leaves the machine.

## Consequences

- Evidence is stronger while remaining privacy-conscious.
- Some visual PiP quality still requires an authorized human/native review, documented as
  such rather than replaced by hashes.
