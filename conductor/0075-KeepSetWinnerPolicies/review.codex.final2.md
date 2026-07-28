# Track Completion Audit — 0075-KeepSetWinnerPolicies

## Verdict: FAIL

## Scope Reviewed

Read-only review of the complete spec/plan, prior final review, current dirty worktree, implementation, tests, CLI/GUI wiring, docs, governance, and verification artifacts.

No files or Git state were modified.

## Requirement and DoD Matrix

| DoD | Status | Evidence |
|---|---|---|
| 1–1b | Met | Reader/scan date and BCC capture; ranking/statistics wired. |
| 2 | Met | Classifier now selects global minimum rank; overlapping-path tests present. |
| 3–5 | Met | Source rank, inversion, ladder metadata, CSV append-only fields implemented. |
| 6 | Partial | Implementation wiring exists, but production three-surface parity is not tested end-to-end. |
| 6b–9 | Met | Signal-only recoverable stat, graded fidelity, CLI surfaces, and GUI mapping present. |
| 10 | Partial | Winner golden exists, but no checked-in pre-0075 row-data baseline is compared. |
| 11–13 | Met | Determinism, SHA-256 immutability, and required documentation present. |
| 14 | Partial | Required test areas exist, but the parity and compatibility tests are insufficiently strong. |
| 15 | Met by orchestrator report; not live-verifiable here | Cached Ledgerful report records all full-gate commands passing. |
| 16 | Intentionally open | Excluded from failure per request. |

## Findings

[P2] Pre-0075 decision CSV compatibility is still not proven

Confidence: High  
Requirement: DoD-10, DoD-14; spec §3.9  
Location: [keep_set.rs](C:/dev/Dedupe/crates/pst-dedup-cli/tests/keep_set.rs:632)

Problem: `ASPOSE_DEFAULT_WINNER_GOLDEN` is checked in, but `DECISION_CSV_PRE_0075_HEADER` freezes only the header. The test asserts current unique-row NIDs from the first 19 columns, not byte-identical legacy row data against a checked-in baseline.

Failure scenario: `MessageIdNorm`, hash, policy, degraded fields, size, or row content could regress while winner NIDs and the header remain unchanged; the test would pass.

Correction: Check in the pre-0075 unique-row legacy-column baseline and compare all first-19-column values byte-for-byte.

Verification: Run `aspose_default_winners_deterministic_golden` after adding the baseline comparison.

Deferrable: No

[P2] All-Custodians parity is still not proven through the production unique-PST path

Confidence: High  
Requirement: DoD-6, DoD-14; spec §3.7/test item 8  
Locations: [keepset.rs](C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:3882), [unique_pst_cmd.rs](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:1650), [unique_pst.rs](C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst.rs:143)

Problem: The engine test derives the “export” values directly from `KeepEntry`; the CLI test checks only export headers and row counts. No multi-source production `unique-pst` test parses and compares the decision CSV unique row, keep-set JSON winner, and `export_messages.csv`.

Failure scenario: The production lookup/fill can emit zero or mismatched aggregate values while the current tests remain green.

Correction: Add a synthetic multi-source `unique-pst` integration test with more than eight source basenames, parsing all three artifacts and asserting exact count, capped sorted basenames, and truncation semantics.

Verification: Run the new integration test and verify source PST SHA-256 immutability.

Deferrable: No

## Completeness Sweep

No new production placeholders, stubs, fake-success paths, secrets, or unrelated scope defects were found. The folder classifier residual is fixed, including `Recoverable Items/Purges/Sent Items -> sent_items` and pure `Purges -> recoverable_purges`.

## Wiring and Regression Review

The core path is wired:

`pst-reader -> scan -> RecoverableScanItem -> RankContext -> keep-set -> decision CSV / JSON -> unique-pst export`.

The production export fields are present, but the required end-to-end parity proof remains missing.

## Verification Evidence

Observed now:

- `git diff --check`: passed.
- `cargo fmt --all --check`: passed.
- Targeted Cargo tests and clippy could not execute here because `C:\dev\Dedupe\target\debug\.cargo-build-lock` returned `Access is denied`.
- `ledgerful ledger status --compact`: failed with `unable to open database file`.
- Cached `latest-verify.json` records fmt, workspace clippy, and workspace tests exit code 0; this is cached/orchestrator evidence, not a live execution in this review.
- Orchestrator-reported: selected clippy green, dedup-engine 121 tests passed, keep-set 12 tests passed.

## Deferred Candidates

None. DoD-16 is intentionally open and is not the failure reason. The remaining issues are P2 and cannot be deferred.

## Completion Decision

FAIL. The folder-class P2 is fixed, but the compatibility baseline and production three-surface All-Custodians parity P2s remain unresolved.