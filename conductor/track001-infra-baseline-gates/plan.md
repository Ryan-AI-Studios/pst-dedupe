# Track 001 Plan: Infra Baseline Gates

## Objective

Make the workspace consistently compile, test, and pass baseline repository gates before deeper PST functionality work begins.

## Scope

- Keep `cargo check --workspace` and `cargo test --workspace` passing.
- Repair formatting drift so `cargo fmt --all --check` passes.
- Decide which compiler warnings are acceptable during active implementation and remove the rest.
- Align ChangeGuard verification with real Cargo commands.
- Record the verified baseline in the conductor.

## Steps

1. Run the current baseline commands and capture failures.
2. Apply repo-wide Rust formatting.
3. Fix warnings that indicate dead code, unused imports, stale APIs, or misleading public surface.
4. Update ChangeGuard verify configuration if it references nonexistent commands.
5. Audit dependency pins for stale or breaking-version drift before changing code.
6. If pins are updated, read release notes and handle syntax/API migrations in the same track.
7. Re-run all baseline checks.
8. Update this track with final verification notes.

## Hardening Notes

- Do not mask warnings with broad crate-level `allow` attributes.
- Keep MSRV and Windows compatibility visible when dependency pins move.
- Treat a new warning as a regression unless it is explicitly accepted in the conductor.
- See [Track Guardrails](../TRACK-GUARDRAILS.md).

## Verification Notes

Verified on 2026-05-15 (commit `b544a4e` area):

- `cargo fmt --all --check` — pass (no diff).
- `cargo clippy --workspace --all-targets -- -D warnings` — pass (no warnings).
- `cargo test --workspace` — pass (13 unit tests + 1 doc test across dedup-engine, pst-reader, pst-dedup-gui).
- `cargo check -p pst-dedup-gui` — pass.
- `changeguard verify` — pass (exit 0). Prediction-history warnings are accepted debt (new repo, <10 commits).

ChangeGuard `verify.steps` configuration alignment (currently commented out in `.changeguard/config.toml`) is scoped to Track 006.

## Exit Criteria

- `cargo fmt --all --check` passes.
- `cargo check --workspace` passes.
- `cargo test --workspace` passes.
- ChangeGuard verification either passes or has documented, actionable remaining blockers.
