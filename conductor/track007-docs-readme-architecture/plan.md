# Track 007 Plan: README And Architecture Refresh

## Objective

Provide accurate user/developer documentation for the current PST dedupe project state.

## Scope

- Add `README.md`.
- Clean encoding artifacts in `ARCHITECTURE.md`.
- Update stale dependency versions and implementation status.
- Document limitations honestly.

## Steps

1. Draft README with purpose, build, test, and current status.
2. Repair architecture encoding artifacts.
3. Update dependency table to match workspace dependencies.
4. Add fixture and verification instructions.
5. Add dependency update policy, including release-note review and syntax/API migration expectations.
6. Document known limitations and edge cases without overstating completion.

## Hardening Notes

- Docs must distinguish implemented, tested, and planned behavior.
- Dependency versions in docs must match the workspace.
- PST privacy and fixture handling must be explicit.
- See [Track Guardrails](../TRACK-GUARDRAILS.md).

## Exit Criteria

- New contributors can understand what works and what is pending.
- Docs no longer claim completed functionality that is still unproven.
