# Track Completion Audit — 0093-WriterHeapRecipientRobustness r3

## Verdict: FAIL

## Scope Reviewed

Current staged, unstaged, and untracked worktree; full `spec.md`/`plan.md`; r1/r2 findings; writer, CLI aggregation, summary serialization, QC parser, tests, docs, and deferred records.

## Requirement and DoD Matrix

| Requirement | Status | Evidence |
|---|---|---|
| DoD-1 cumulative helper diversion | Met | Adaptive reprobe and multi-helper round-trip test |
| DoD-2 recipient Strategy B/QC honesty | Partial | Writer path is correct, but malformed event validation remains incomplete |
| DoD-3 residuals and D-0068-01 | Met | Deferred entries and closure documentation present |
| DoD-4 engineering gates | Met as reported | `cargo fmt --all --check` independently passed; other gates not rerun due read-only scope |
| DoD-5 governance | Deferred as instructed | `review.md`/Completed/ledger closure remain unfinished; not used as a failure basis |

## Prior Finding Verification

- r1 fail-open defaults: fixed; strict deserialization and reason validation are present.
- r2 ordinary overdrop: fixed at [unique_pst_qc.rs:2591](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:2591), with regression coverage at [unique_pst_qc.rs:3622](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:3622).
- The fresh sweep found additional fail-closed gaps.

## Findings

[P1] QC accepts overflowing class totals through saturating arithmetic

Confidence: High  
Requirement: DoD-2; malformed events must remain a defect  
Location: [unique_pst_qc.rs:2578](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:2578)  
Problem: `saturating_add` can turn impossible totals into `u32::MAX`, allowing them through validation.  
Evidence: `source_count=u32::MAX`, `kept_count=0`, and all three dropped-class counts `u32::MAX` pass the current checks.  
Failure scenario: A malformed summary event can be accepted as a valid truncation explanation and produce `KnownGap`.  
Correction: Use checked arithmetic or explicit non-overflowing bounds for every aggregate.  
Verification: Add a maximum-value overflow regression test and rerun QC tests.  
Deferrable: No

[P1] QC does not bind `event.source_count` to the actual source recipient set

Confidence: High  
Requirement: DoD-2; unexplained recipient loss must remain `Defect`  
Location: [unique_pst_qc.rs:1718](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1718)  
Problem: QC only checks `out_written == event.kept_count`; it never checks that `event.source_count` equals the policy-filtered source count.  
Evidence: The parser accepts `source_count=1, kept_count=0, dropped_to=1`; QC then classifies an output with zero rows as `KnownGap` even if the actual source contains 136 rows.  
Failure scenario: A corrupted or mismatched event suppresses a real recipient-loss defect.  
Correction: Require `event.source_count == src_keys.len()` before classifying `KnownGap`, with a regression test for mismatched source totals.  
Verification: Run the clean-room QC regression suite.  
Deferrable: No

## Completeness Sweep

No new production placeholders, stubs, disconnected paths, or silent Display* truncation were found. The writer event path is wired through counters, `summary.json`, and live/clean-room QC.

## Verification Evidence

Observed:

- `cargo fmt --all --check` — PASS
- `git diff --check HEAD` — PASS
- `cargo metadata --no-deps` — PASS
- Ledgerful status/doctor — unavailable: database could not be opened
- ai-brains — unavailable: vault key missing
- Cargo tests/clippy — not rerun because the review was explicitly read-only

## Completion Decision

FAIL. The r2 finite overdrop case is closed, but DoD-2 still has two P1 fail-open validation paths.