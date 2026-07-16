# TASK-005: External public-release gates

- Status: blocked_external
- ADRs: [ADR-0001](../adr/ADR-0001-portable-windows-release.md)

## Required external inputs

- A real Authenticode signing identity and secure CI secret strategy.
- A disposable clean Windows VM snapshot.
- Permission to install/uninstall drivers and exercise UAC denial/approval.

## Proof still required

- [ ] Signed binaries and archive verified with the intended trust chain.
- [ ] Installer format selected after MSI/MSIX experiment.
- [ ] Fresh install, repair, upgrade, uninstall, and rollback.
- [ ] Unity backend download/hash/register and UAC denial/approval recovery.
- [ ] OBS and Unity receiver evidence on the clean machine.
- [ ] Published tag/release workflow and download verification.

This task is intentionally not silently converted into a repository-only checkbox.
