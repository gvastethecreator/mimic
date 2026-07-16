# TASK-003: Release engineering

- Status: done
- ADRs: [ADR-0001](../adr/ADR-0001-portable-windows-release.md),
  [ADR-0002](../adr/ADR-0002-pinned-toolchain-and-dependencies.md)

## Checklist

- [x] Add focused local check and full release-gate scripts.
- [x] Bootstrap a coherent MSVC/Windows SDK environment from a normal PowerShell.
- [x] Add Windows CI with pinned toolchain and cache-safe build steps.
- [x] Enforce target-aware audit policy.
- [x] Build a versioned portable directory and ZIP.
- [x] Generate and verify SHA-256 manifest and provenance.
- [x] Add optional fail-closed Authenticode signing and verification.
- [x] Run package smoke from extracted contents.
- [x] Document build, verify, sign, and uninstall flows.

## Evidence

`scripts/package.ps1` produced identical SHA-256 output from identical inputs, and
`scripts/verify-package.ps1` validated manifest/provenance, CLI version, and a packaged
environment check from a fresh extraction. CI calls the same release gate. The artifact
truthfully records `signed: false` until TASK-005 receives external inputs.
