# Track Completion Audit — 0093-WriterHeapRecipientRobustness

## Verdict: FAIL

## Scope Reviewed

Reviewed `origin/main` through the working tree, including staged, unstaged, and untracked changes. Read all of `spec.md` and `plan.md`, implementation paths, tests, docs, deferred records, and governance state.

## Requirement and DoD Matrix

| Requirement | Status | Evidence |
|---|---|---|
| DoD-1 cumulative helper diversion | Met | Adaptive escalation/re-probe in `production.rs`; multi-helper round-trip test |
| `message_class` diversion | Met | Included in helper PID set and production helper |
| No Display* clipping | Met | Subnode diversion; round-trip tests |
| DoD-2 budget-aware recipient cap | Met | Retry-based cap can keep `<48`; actual count emitted |
| To>Cc>Bcc stable ordering | Met | Explicit ordering before cap |
| Counters/events and summary wiring | Met | Writer report → CLI aggregation → `summary.json` |
| QC KnownGap behavior | Partial | Normal path works, but malformed summary events fail open; Finding P1 below |
| `recipient_table` remains Preserved | Met | Contract unchanged |
| DoD-3 residuals and D-0068-01 | Met | Deferred records present; D-0068-01 marked closed |
| DisplayTo QC sampling | Met | `max_by_key(display_to.len())` added and tested |
| DoD-4 gates | Reported met | `fmt` independently observed passing; clippy/tests reported by orchestrator, not rerun |
| DoD-5 governance | Unmet | No canonical `review.md`; conductor remains In Progress; no track commit observed |

## Findings

[P1] Clean-room QC accepts malformed truncate records as valid KnownGap events  
Confidence: High  
Requirement: DoD-2; unexplained row loss must remain `Defect`  
Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:2550)  
Problem: `load_recipient_tc_truncations_for_qc` silently defaults required fields to zero and does not validate the serialized reason. A malformed summary entry can therefore produce an apparently valid truncate event.  
Evidence: `unwrap_or(0)` is used for `kept_count` and all counters; QC classifies matching events as `KnownGap` when `out_written == kept_count`.  
Failure scenario: A summary event missing `kept_count` defaults to zero, and an output with zero written recipients can be classified as `KnownGap` instead of unexplained loss.  
Correction: Strictly deserialize and validate `reason`, required fields, count relationships, and per-class totals; reject malformed records fail-closed.  
Verification: Add malformed-summary tests covering missing fields, invalid reason, and inconsistent counts.  
Deferrable: No

[P1] Track completion governance is incomplete  
Confidence: High  
Requirement: DoD-5  
Location: [plan.md](/C:/dev/Dedupe/conductor/0093-WriterHeapRecipientRobustness/plan.md:45)  
Problem: The canonical completion artifacts are absent. `review.md` does not exist, `conductor.md` still says `In progress`, sequencing still says `Ready`, and `git log origin/main..HEAD` is empty. `ledgerful ledger status --compact` could not run because its database could not be opened.  
Correction: Orchestrator must write canonical `review.md`, update conductor status, complete the Ledgerful commit, and record exact verification results.  
Verification: `ledgerful ledger status --compact`; `ledgerful verify`; confirm `Completed` governance state.  
Deferrable: No

## Completeness Sweep

No track-specific production stubs, no-op paths, or placeholder implementations were found. Production changes use `Result`; `unwrap`/`expect` matches found in the affected areas are test-only.

## Wiring and Regression Review

The writer event path is reachable and correctly aggregated into `summary.json`. Live QC and clean-room `qc-pst` both consume the event. Recipient ordering, actual kept counts, counters, Display* preservation, and residual documentation are wired consistently.

The clean-room summary parser is the material integrity exception noted above.

## Verification Evidence

Observed:

- `cargo fmt --all --check` — PASS
- `git diff --check origin/main` — PASS

Reported by orchestrator:

- Targeted clippy — PASS
- `cargo test -p pst-writer` — PASS
- `unique_pst_qc_0080` — 58 PASS
- `unique_pst` — 31 PASS

Not independently observed:

- Full `cargo clippy --workspace`
- Full `cargo test --workspace`
- `ledgerful verify`

## Deferred Candidates

None. The findings are not eligible for P3 deferral.

## Completion Decision

FAIL. Fix the fail-open clean-room event parsing, then complete DoD-5 governance and Ledgerful provenance.