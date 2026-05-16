# Track 006 Plan: Quality Gates Repair

## Objective

Make local and ChangeGuard quality gates match the real project commands.

## Scope

- Fix stale ChangeGuard verify commands.
- Document the canonical local verification sequence.
- Keep gate output actionable for future tracks.

## Steps

1. Inspect ChangeGuard config and hooks.
2. Replace nonexistent commands with Cargo workspace commands.
3. Add explicit gates for formatting, checking, tests, and dependency pin review where supported.
4. Confirm gates behave correctly with missing fixtures and dirty unrelated files.
5. Run verification and update conductor notes.

## Hardening Notes

- Verification must fail for real regressions but not for absent private fixtures.
- Gate commands should be stable on Windows PowerShell.
- Pin-update gates must catch breaking APIs early without requiring network during every run.
- See [Track Guardrails](../TRACK-GUARDRAILS.md).

## Exit Criteria

- ChangeGuard verify no longer fails because of missing commands.
- The repo has one documented baseline command sequence.
