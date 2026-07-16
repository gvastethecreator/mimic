# ADR-0001: Portable Windows release before installer

- Status: Accepted
- Date: 2026-07-15
- Owners: Mimic maintainers

## Context

Mimic has no release artifact, signature, installer, or clean-machine uninstall proof.
It also manages virtual-camera backends that may require elevation. This workstation has
no signing certificate or installed MSI/MSIX authoring toolchain.

## Decision

The first reproducible release artifact is a Windows x64 portable ZIP containing the GUI,
the diagnostic console binary, license/readme material, provenance, and a SHA-256
manifest. Packaging must be deterministic enough to rebuild and self-verify locally.

Authenticode signing is an optional, fail-closed step: when explicitly enabled, every
executable is signed and verified with SignTool before archiving. Unsigned packages are
clearly named and documented; CI must never imply that they are signed.

MSI versus MSIX remains undecided until a clean Windows VM proves installation,
elevation denial/approval, virtual-camera registration, upgrade, repair, and uninstall
with a real signing identity.

## Consequences

- A useful artifact can be produced and tested without fake trust claims.
- Portable uninstall is explicit: stop output, close Mimic, then remove its directory;
  backend removal remains an in-app/system operation and must be documented separately.
- Public distribution remains gated on signing policy and clean-machine proof.
