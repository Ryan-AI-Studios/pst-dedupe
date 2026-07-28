# Track Completion Audit — 0075-KeepSetWinnerPolicies

## Verdict: FAIL

## Scope Reviewed

Read-only review of:

- Full `spec.md` and `plan.md`
- Working tree versus `origin/main`
- Prior `review.codex.md`
- Engine, reader, CLI, GUI, tests, docs, and governance
- Fresh completeness and regression sweep

No files or Git state were modified.

## Fix-Round Disposition

| Finding | Result |
|---|---|
| Optional delivery/BCC reads soft-fail | Fixed in scan path: `message.rs:181-190` |
| Folder classifier minimum-rank selection | Partially fixed; residual mixed recoverable/non-recoverable edge remains |
| Pre-1970 FILETIME formatting | Fixed; signed civil-date formatter and test present |
| Human honesty statistics | Fixed; all three summaries print all counters |
| All-Custodians three-surface parity tests | Residual; export surface is not compared to engine/JSON/decision CSV |
| Fixture golden/header/SHA immutability | Residual; test compares two runs of the same implementation, not a checked-in baseline |
| `cargo fmt` | Fixed; observed passing |

## Requirement and DoD Matrix

| DoD | Status | Evidence / Gap |
|---|---|---|
| DoD-1 | Met, execution not verifiable | Date/BCC capture and ranking are wired through reader → scan → engine |
| DoD-1b | Met, execution not verifiable | BCC rung and always-on loss statistic implemented with unit coverage |
| DoD-2 | Partial | Folder ladder exists, but global precedence has a residual bug below |
| DoD-3 | Met, execution not verifiable | Ordered source rank, unmatched-worst behavior, inversion, and fixture test are present |
| DoD-4 | Partial | Ladder structure is present; mixed-folder classification is incomplete |
| DoD-5 | Met | Append-only decision columns and closed vocabularies are implemented |
| DoD-6 | Partial | Aggregate is wired to all three outputs, but parity is not proven end-to-end |
| DoD-6b | Met | Recoverable-items signal is computed independently of ranking |
| DoD-7 | Met, execution not verifiable | Graded mapping is exhaustive and binary compatibility is tested |
| DoD-8 | Met | CLI surfaces expose the new flags and policy parsers |
| DoD-9 | Met | GUI policy, folder, and BCC controls plus mapping test exist |
| DoD-10 | Partial | Pre-0075 JSON and header prefix tests exist; required fixture baseline is absent |
| DoD-11 | Met, execution not verifiable | Shuffled-input determinism test exists |
| DoD-12 | Met, execution not verifiable | SHA-256 immutability assertions exist |
| DoD-13 | Met | Documentation covers policies, honesty limits, folder ladder, and provenance |
| DoD-14 | Partial | Required parity and baseline regression proof remain insufficient |
| DoD-15 | Not verifiable | `fmt` passed; clippy/tests were blocked by Cargo lock access |
| DoD-16 | Intentionally open | Per request, not treated as a failure |

## Findings

[P2] Folder classification still short-circuits before global ladder precedence

Confidence: High  
Requirement: DoD-2, DoD-4; spec §3.4  
Location: `crates/dedup-engine/src/keepset.rs:911-923`, `:970-984`

Problem: `classify_folder` returns `classify_recoverable()` before evaluating non-recoverable segments. Thus a path such as `Recoverable Items/Purges/Sent Items` is classified as `recoverable_purges` (rank 9), although `Sent Items` is a valid rank-0 match.

The fix correctly chooses the minimum rank within each category, but not across all matching classes.

Correction: Collect recoverable and non-recoverable matches together, then select the minimum `builtin_rank`; retain Recoverable Items parent qualification.

Verification: Add overlapping tests such as `Recoverable Items/Purges/Sent Items` and assert `sent_items`.

Deferrable: No

[P2] Fixture “golden” regression is still not a checked-in compatibility baseline

Confidence: High  
Requirement: DoD-10, DoD-14; spec §3.9; plan Phase 7  
Location: `crates/pst-dedup-cli/tests/keep_set.rs:629-735`

Problem: The test runs the current implementation twice and compares the two outputs. It does not compare winners against a checked-in golden winner list, nor does it compare the decision CSV’s 19 pre-0075 data columns byte-for-byte against a pre-0075 baseline.

The test does verify determinism, header prefix presence, and fixture SHA-256 immutability, but it would pass even if both runs had the same compatibility regression.

Correction: Add a checked-in fixture winner list and a frozen pre-0075 decision-column baseline, then compare current default output against both.

Verification: Run the fixture regression and assert exact winner keys plus exact legacy-column output.

Deferrable: No

[P2] All-Custodians parity is not proven across the three production artifacts

Confidence: High  
Requirement: DoD-6; spec §3.7 and test item 8  
Location: `crates/dedup-engine/src/keepset.rs:3869-3928`; `crates/pst-dedup-cli/src/unique_pst_cmd.rs:1650-1679`; `crates/pst-dedup-cli/tests/unique_pst.rs:143-190`

Problem: Engine coverage compares `KeepEntry` to a `DecisionRecord`, while the export-report test constructs an `ExportMessageRow` directly. No test executes the production unique-PST path and compares exact count, sorted capped basename list, and truncation semantics across:

- Decision CSV unique row
- `keep_set_v1` JSON winner
- `export_messages.csv`

Correction: Add a synthetic multi-source CLI integration test that parses all three artifacts and asserts exact equality, including cap-8 and basename-only behavior.

Verification: Run the production export path with more than eight duplicate source basenames.

Deferrable: No

## Completeness Sweep

The core path is wired end-to-end:

`PST reader → scan candidates → RankContext → keep-set resolution → decision CSV / JSON / export report / human summaries`.

No new production placeholder, stub, fake-success path, or unrelated secret file was found. CSV append-only behavior and formula neutralization remain present.

## Verification Evidence

Observed:

- `git diff --check` — passed
- `cargo fmt --all --check` — passed
- Built debug binary keep-set smoke — succeeded; human output included all three honesty counters
- Built debug `--help` — exposed all requested policy flags
- `ledgerful verify` — fmt passed; clippy and workspace tests failed because `C:\dev\Dedupe\target\debug\.cargo-build-lock` returned `Access is denied`
- Direct targeted clippy/tests — same Cargo lock failure
- `ledgerful ledger status --compact` — `unable to open database file`
- `ledgerful scan --impact` — could not write impact report
- Cached impact report matched current HEAD, showed dirty tree and high risk

The orchestrator-reported green gates were not independently claimable under the read-only environment.

## Deferred Candidates

None. DoD-16 remains intentionally open for orchestrator closeout and is not itself a failure reason.

## Completion Decision

FAIL due to three residual P2 issues: incomplete global folder-rank precedence, inadequate default compatibility golden proof, and missing three-surface All-Custodians parity testing.