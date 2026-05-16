# pst-dedupe Conductor

This board tracks bounded implementation work. Keep entries short and update them when work starts or finishes.

## Status Legend

- `Planned`: scoped but not started.
- `Active`: currently being implemented.
- `Blocked`: cannot proceed without an external decision or dependency.
- `Complete`: implemented and verified.

## Tracks

| Track | Status | Area | Objective | Notes |
|---|---|---|---|---|
| 001 | Planned | Infra | Make workspace compile and establish baseline Cargo/ChangeGuard gates. | GUI compile blocker resolved during dependency refresh. Remaining known blocker: pst-reader permute crypto test failure. |
| 002 | Planned | Reader | Add real PST fixture strategy and prove `PstFile::open` plus folder/message traversal. | Required before claiming PST dedup is functional. |
| 003 | Planned | Dedup | Correct tier semantics and add tests for Tier 1/Tier 2 behavior. | Tier 2 disabled and fallback-only behavior need coverage. |
| 004 | Planned | GUI | Make scan errors, partial results, and EML export behavior honest. | Export unique EML is currently not wired through the GUI. |

## Operating Notes

- Use ChangeGuard ledger transactions for meaningful changes.
- Pin durable decisions in ai-brains.
- Do not mark tracks complete without verification notes.
