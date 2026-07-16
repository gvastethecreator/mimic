# Release-readiness task ledger

This ledger is the durable execution source of truth for the post-recovery release
mission. Update status and evidence in the same change as the implementation.

Status vocabulary: `planned`, `in_progress`, `blocked_external`, `done`.

| Task | Outcome | Status | Evidence |
|---|---|---|---|
| [TASK-001](TASK-001-foundations.md) | Toolchain, dependencies, metadata, identity | done | Pinned build, focused tests, clippy, release binaries |
| [TASK-002](TASK-002-diagnostics.md) | Scriptable diagnostics and test seams | done | CLI contract plus real media/camera/receiver/soak reports |
| [TASK-003](TASK-003-release-engineering.md) | CI, audit policy, package, signing hook | done | Local release gate, reproducible ZIP, extracted smoke |
| [TASK-004](TASK-004-product-proof.md) | Native UI, receiver, camera, soak evidence | done | [Dated evidence report](../release-readiness-2026-07-15.md) |
| [TASK-005](TASK-005-external-release-gates.md) | Signing and clean-machine installer lifecycle | blocked_external | Needs certificate and clean Windows VM |

## Release-candidate definition

The repository may call a commit a *release candidate* only when TASK-001 through
TASK-004 are `done`, the final gate is green, and TASK-005 is still described as an
external distribution gate. It may not call the artifact a signed public release until
TASK-005 is proven.
