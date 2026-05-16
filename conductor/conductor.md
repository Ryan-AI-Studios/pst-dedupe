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
| 001 | Complete | Infra | Make workspace compile and establish baseline Cargo/ChangeGuard gates. | [Details](track001-infra-baseline-gates/plan.md). Verified 2026-05-15: `cargo fmt`, `cargo clippy`, `cargo test --workspace`, and `changeguard verify` all pass. ChangeGuard config alignment deferred to Track 006. |
| 002 | Complete | Reader | Add real PST fixture strategy and prove `PstFile::open` plus folder/message traversal. | [Details](track002-real-pst-fixtures-traversal/plan.md). Verified 2026-05-15: Aspose sample PST fixture (271 KB) opens and traverses. Magic constant bug (`PST_MAGIC` endianness) fixed. 6 integration tests pass. |
| 003 | Complete | Dedup | Correct tier semantics and add tests for Tier 1/Tier 2 behavior. | [Details](track003-dedup-tier-semantics/plan.md). Verified 2026-05-15: Tier 2 configurable via `DedupIndex::with_tier2`. Tier 1 priority enforced. 18 dedup-engine tests pass. |
| 004 | Complete | GUI | Make scan errors and partial results visible to the user. | [Details](track004-gui-errors-partial-results/plan.md). Verified 2026-05-15: per-file error tracking, skipped message counts, partial-results warning banner in results view. |
| 005 | Complete | Export | Wire unique-message EML export end to end. | [Details](track005-export-unique-eml/plan.md). Verified 2026-05-15: EML export re-opens PSTs, writes unique messages, shows export feedback. |
| 006 | Complete | Infra | Repair baseline quality gates. | [Details](track006-quality-gates-repair/plan.md). Verified 2026-05-15: ChangeGuard verify now runs `fmt`, `clippy`, `test` sequentially. Rules updated to match real step names. |
| 007 | Complete | Docs | Add user-facing README and refresh architecture notes. | [Details](track007-docs-readme-architecture/plan.md). Verified 2026-05-15: README.md added, ARCHITECTURE.md magic constant and dependency versions corrected. |
| 008 | Complete | Reader | Harden PST format correctness. | [Details](track008-pst-reader-hardening/plan.md). Verified 2026-05-15: CRC checks added (warning-only pending algorithm verification), block reads unified through `read_raw_block`, corrupted-PST handling avoids hard failures. |
| 009 | Complete | Release | Prepare Windows executable packaging. | [Details](track009-windows-release-packaging/plan.md). Verified 2026-05-15: `cargo build --release -p pst-dedup-gui` produces a ~13 MB self-contained executable; smoke test passed (process starts and runs without missing DLLs); release instructions added to README. |
| 010 | Complete | Security | Address HIGH/MEDIUM audit findings from comprehensive code audit. | [Details](track010-audit-hardening/plan.md). Verified 2026-05-16: mutex poisoning recovery, Unicode-safe truncation, EML filename hardening, FILETIME/format_size deduplication. 33 tests pass. |

## Operating Notes

- Use ChangeGuard ledger transactions for meaningful changes.
- Pin durable decisions in ai-brains.
- Do not mark tracks complete without verification notes.
