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
| 001 | Planned | Infra | Make workspace compile and establish baseline Cargo/ChangeGuard gates. | GUI compile blocker and pst-reader permute crypto failure resolved. Remaining cleanup: formatting and warning baseline. |
| 002 | Planned | Reader | Add real PST fixture strategy and prove `PstFile::open` plus folder/message traversal. | Required before claiming PST dedup is functional. |
| 003 | Planned | Dedup | Correct tier semantics and add tests for Tier 1/Tier 2 behavior. | Tier 2 disabled and fallback-only behavior need coverage. |
| 004 | Planned | GUI | Make scan errors and partial results visible to the user. | Worker currently logs or stores some errors without clear result-state reporting. |
| 005 | Planned | Export | Wire unique-message EML export end to end. | GUI button chooses a folder but does not re-read PSTs or call the exporter. |
| 006 | Planned | Infra | Repair baseline quality gates. | `cargo fmt --all --check` fails repo-wide; warnings remain; `changeguard verify` references nonexistent `build`. |
| 007 | Planned | Docs | Add user-facing README and refresh architecture notes. | No `README.md`; `ARCHITECTURE.md` has encoding artifacts and stale dependency versions. |
| 008 | Planned | Reader | Harden PST format correctness. | Add CRC checks, corrupted PST handling, named-property fallback, and large-file coverage. |
| 009 | Planned | Release | Prepare Windows executable packaging. | Needs icon/metadata, release profile validation, and deployment instructions after functional proof. |

## Operating Notes

- Use ChangeGuard ledger transactions for meaningful changes.
- Pin durable decisions in ai-brains.
- Do not mark tracks complete without verification notes.
