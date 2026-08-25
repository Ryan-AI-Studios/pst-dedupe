# Track Completion Audit — 0093-WriterHeapRecipientRobustness

## Verdict: FAIL

## Scope Reviewed

Read-only review of `spec.md`, `plan.md`, prior r1 review, implementation/tests, docs, deferred records, staged/unstaged/untracked changes, and cached Ledgerful impact data.

## Requirement and DoD Matrix

| Requirement | Status | Evidence |
|---|---|---|
| DoD-1 cumulative helper diversion | Met | Adaptive reprobe and multi-helper round-trip test |
| DoD-2 recipient Strategy B | Partial | Writer/event/QC wiring is present, but malformed events can still be accepted |
| DoD-3 residuals and D-0068-01 | Met | D-0068-01 closed; both 0093 residuals recorded |
| DoD-4 engineering gates | Met as reported | fmt observed; remaining gates reported by orchestrator |
| DoD-5 governance | Deferred per handoff | Not considered a failure for this review |

## Findings

### P0

None.

### P1

[P1] Clean-room parser still accepts impossible recipient class totals  
Confidence: High  
Requirement: DoD-2; malformed truncate events must fail closed  
Location: [unique_pst_qc.rs:2578](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:2578)  
Problem: Validation only rejects class totals that exceed broad totals. It does not require dropped-class totals to fit within `source_count - kept_count`. For example, `source_count=10`, `kept_count=3`, `kept_to=0`, `dropped_to=10` is accepted even though it describes 13 rows.  
Evidence: `kept_classes + dropped_classes > source_count` passes for `0 + 10`; per-class validation also passes. The matching QC path then classifies an output with three rows as `known_gap`.  
Failure scenario: A corrupted or malicious summary event can suppress a real recipient-loss defect.  
Correction: Validate `dropped_to + dropped_cc + dropped_bcc <= source_count - kept_count` using non-saturating arithmetic; add undercount/overdrop parser and clean-room tests.  
Verification: Re-run the parser unit test, QC test, and targeted gates.  
Deferrable: No

### P2

None.

### P3

None.

## Completeness Sweep

No new track-specific production stubs, no-op paths, or silent Display* truncation were found. Existing fixture-only placeholders were outside this track’s changed production path.

## Wiring and Regression Review

The writer event path is correctly connected:

`pst-writer counters → unique-pst aggregation → summary.json → live/clean-room QC`.

Cumulative helper diversion, `message_class`, To-first ordering, actual kept counts, full Display* preservation, counters, residual documentation, and sample selection are wired correctly. The parser validation gap above is the remaining integrity exception.

## Verification Evidence

Observed:

- `cargo fmt --all --check` — PASS
- `git diff --check HEAD` — PASS
- Ledgerful status — unavailable: database could not be opened
- ai-brains — unavailable: vault key missing

Reported by orchestrator/user, not independently rerun:

- Targeted clippy — PASS
- Parser regression test — PASS
- Recipient TC QC test — PASS
- Earlier full pst-writer, `unique_pst_qc_0080`, and `unique_pst` tests — PASS

## Deferred Candidates

None. The remaining issue is P1 and cannot be deferred.

## Completion Decision

FAIL. The prior clean-room fail-open finding is only partially closed; class-total validation must be tightened before completion.