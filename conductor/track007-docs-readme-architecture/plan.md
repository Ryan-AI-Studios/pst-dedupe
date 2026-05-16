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

## Verification Notes

Verified on 2026-05-15:

- **`README.md`** added with: purpose, build/test instructions, architecture table, current status matrix, verification gate, and license.
- **`ARCHITECTURE.md`** fixes:
  - Corrected `dwMagic` from `0x2142444E` to `0x4E444221` (little-endian u32).
  - Updated dependency versions to match workspace: sha2=0.11, csv=1.4, eframe=0.34, rfd=0.17, crc32fast=1.5.
  - Removed specific client reference.
- Status matrix in README honestly documents what is implemented vs pending.

## Exit Criteria

- New contributors can understand what works and what is pending.
- Docs no longer claim completed functionality that is still unproven.
