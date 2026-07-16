# ADR-0003: Separate scriptable diagnostics from the GUI

- Status: Accepted
- Date: 2026-07-15

## Context

Mimic's GUI can explain setup failures to a person, but CI and support need stable output,
timeouts, and exit codes. A Windows GUI binary is the wrong place for stdout contracts.

## Decision

Add `mimic-doctor.exe`, a side-effect-free-by-default console binary for humans and
scripts.

Usage contract:

```text
mimic-doctor [--json] <command> [options]

commands:
  check             inspect configuration, FFmpeg, and virtual-camera availability
  media             decode a bounded number of frames from an explicit file
  camera            capture/count bounded frames from an explicit named device
  virtual-output    send deterministic frames and prove an FFmpeg receiver sees them
  soak              exercise a bounded media loop and report timing/resource metrics
```

- Human output is concise; `--json` is stable and writes only JSON to stdout.
- Diagnostics and errors use stderr; no prompts are allowed.
- `0` means the requested proof passed, `2` invalid usage/input, `3` unavailable
  dependency/device, `4` proof failed or timed out, and `1` unexpected failure.
- Hardware/state-changing commands require an explicit command and finite duration.
- Every child process and camera handle has bounded cleanup.

## Consequences

- CI can prove package health without GUI automation.
- Support reports are structured and comparable.
- The application needs library seams shared by both binaries.
