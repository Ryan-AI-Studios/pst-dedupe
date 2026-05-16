# Track 001 Spec: Infra Baseline Gates

## Problem

The repo has moved past the initial compile and PST crypto blockers, but the baseline quality gates are not yet clean. A project this low-level needs a reliable baseline before real PST fixture work and reader hardening are useful.

## Expected Behavior

- A developer can clone the repo and run the standard workspace checks without local guesswork.
- Formatting is deterministic and enforced.
- Warnings are either removed or intentionally deferred in a documented place.
- ChangeGuard verification points at commands that exist in this Rust workspace.
- Dependency pins are current enough to be maintained and not unexpectedly broken by syntax or API drift.

## Edge Cases

- Clean checkout with no local fixture PSTs.
- Dirty worktree containing unrelated user changes.
- Dependency minor or major updates that change Rust syntax, feature names, default features, or MSRV.
- ChangeGuard available but semantic indexing unavailable.

## Non-Goals

- Do not change PST reader semantics except where required to remove obvious compile or test issues.
- Do not rewrite architecture or introduce new dependencies.
- Do not hide warnings with broad `allow` attributes unless the warning is intentional and documented.

## Verification

- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `changeguard verify`
