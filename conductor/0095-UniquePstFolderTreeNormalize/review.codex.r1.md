# Track Completion Audit — 0095-UniquePstFolderTreeNormalize

## Verdict: FAIL

## Scope Reviewed

Branch `track/0095-UniquePstFolderTreeNormalize` against `main` at `850dce2`, including staged, unstaged, and worktree changes. Read `spec.md`, `plan.md`, and `phase0-triage.md`, then traced writer, CLI, QC, tests, docs, and governance.

## Requirement and DoD Matrix

| Requirement | Status | Evidence / gap |
|---|---|---|
| Phase 0 triage | Met | `phase0-triage.md` classifies prefix race, Deleted Items, residual, and sanitization interactions. |
| Leading consecutive alias strip | Met | `parse_folder_path`; parser unit tests cover sentinel and non-sentinel preservation. |
| Stable multi-source pre-seed / D-0070 | Met for `unique-pst` | CLI passes all winner sources through `WritePstOpts.known_source_paths`; writer tests cover message-one prefixing. |
| Lazy preserve `Unique Mail` with real NID | Met | `ensure_residual` allocates on first residual route; empty-preserve tests cover absence. |
| Flat-layout isolation | Met | Flat branch remains eager and bypasses path parsing. |
| QC normalization / Deleted Items | Partial | Alias, sanitization, and Deleted Items handling are wired, but residual/unparseable expected rows still hard-fail QC. |
| DoD-1 fixture matrix | Unmet | Tests are split between writer path tests and synthetic QC tests; no end-to-end writer→QC matrix with per-folder counts. |
| DoD-2 target contract | Met | `docs/unique-pst-export.md` documents the required rules. |
| DoD-3 | Met for scoped CLI behavior | Deferred entry is closed; ledger commit itself is not verifiable. |
| DoD-4 | Not independently verifiable | Supplied gate results are recorded below. |
| DoD-5 | Unmet/not yet finalized | No canonical `review.md`; conductor remains `Ready`; ledger status could not be read. |

## Findings

### [P1] Residual and unparseable winners still fail `folder_tree_structure` QC

Confidence: High  
Requirement: DoD-1 item 5; QC-honesty objective  
Location: [production.rs:2597](C:/dev/Dedupe/crates/pst-writer/src/production.rs:2597), [unique_pst_qc.rs:765](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:765), [unique_pst_qc.rs:2409](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:2409)

Problem: `normalize_folder_path_key` returns an empty key for empty, alias-only, or `..` paths. QC skips those expected rows, while the writer correctly routes them to `Unique Mail`. With only such a message, `expected_folder_counts` is empty and `folder_tree_matches` rejects the message-bearing `Unique Mail` output.

There is also a parity defect: writer parsing routes paths exceeding `MAX_FOLDER_DEPTH` to residual, but `normalize_folder_path_key` still returns a non-empty deep key.

Failure scenario:

- expected row: `folder_path=""`
- writer output: `Root/Top of Personal Folders/Unique Mail`, count 1
- expected map: empty
- QC result: `folder_tree_structure` defect

Correction: Represent known residual-routed expected rows as the residual destination, while preserving the existing rejection of unexplained metadata absence and multi-leaf collapse. Make normalization/classification share the writer’s depth rule.

Verification: Add an end-to-end empty, alias-only, `..`, and over-depth fixture and assert the real QC verdict plus per-folder counts.

Deferrable: No

### [P2] DoD-1 acceptance coverage is not end-to-end

Confidence: High  
Requirement: DoD-1 and Phase 2 fixture matrix  
Location: [writer_fidelity.rs:333](C:/dev/Dedupe/crates/pst-writer/tests/writer_fidelity.rs:333), [unique_pst_qc.rs:3353](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:3353)

Problem: Writer tests cover prefixing and residual folder materialization, while QC tests use synthetic `VolumeStructuralDigest` values. The required Deleted Items, recoverable-items, non-sentinel provenance, sanitization, and residual cases are not jointly exercised through writer output and production QC with per-folder counts.

Failure scenario: A writer path/NID regression can pass the writer tests while the actual QC matcher still rejects or misclaims the generated tree.

Correction: Add the five specified preserve fixtures, generate/read the output PST, construct the corresponding export rows, and assert matcher verdict and exact per-folder counts.

Verification: Run the targeted fixture suite plus `unique_pst_qc_0080` and `unique_pst` tests.

Deferrable: No

### [P2] Public writer fidelity documentation still describes superseded behavior

Confidence: High  
Requirement: Docs/API/contracts must agree  
Location: [production.rs:1341](C:/dev/Dedupe/crates/pst-writer/src/production.rs:1341), [pst-writer-fidelity-v1.md:78](C:/dev/Dedupe/docs/pst-writer-fidelity-v1.md:78), [pst-writer-fidelity-v1.md:151](C:/dev/Dedupe/docs/pst-writer-fidelity-v1.md:151)

Problem: Public Rust documentation still says residual `Unique Mail` exists at plan start and that D-0070 remains an open streaming residual. `docs/pst-writer-fidelity-v1.md` likewise depicts an always-present `Unique Mail` folder and unresolved prefix race. This conflicts with the implemented scoped contract and `docs/unique-pst-export.md`.

Correction: Update the public writer comments and fidelity document to state lazy preserve residual allocation and conditional pre-seeding via `known_source_paths`.

Verification: Documentation search plus targeted tests and final gates.

Deferrable: No

## Completeness Sweep

No new TODO, FIXME, stub, placeholder, fake-value, or no-op implementation was found in the touched production paths. Core CLI→writer→PST→QC wiring is reachable. No new security or protected-source overwrite regression was evident.

## Wiring and Regression Review

- `unique-pst` collects all winner source paths before streaming writes.
- `IncrementalFolderPlan` pre-seeds prefixes and allocates residual folders with actual NIDs.
- Expected and output QC slots use the shared writer normalization.
- Deleted Items with messages is claimable.
- Flat layout remains isolated from alias stripping and prefix logic.
- The residual QC mismatch and missing integrated fixture coverage remain blocking.

## Verification Evidence

Reported by orchestrator:

- `cargo test -p pst-writer` passed.
- `cargo clippy -p pst-writer -p pst-dedup-cli --all-targets -- -D warnings` passed.
- `unique_pst_qc_0080`: 58 passed.
- Targeted folder-tree tests passed.
- `unique_pst`: 31 passed.
- `cargo fmt --all --check` passed.

Not independently run because this was a read-only audit. `ledgerful ledger status --compact` failed with:

`rusqlite_migration error ... unable to open database file`

Operator INC0102784 re-smoke and ledger commit were not verifiable. `git diff --check` also reports trailing whitespace in new `spec.md:28`; this is not classified as a substantive finding.

## Deferred Candidates

None. Findings are P1/P2 or straightforward contract/test gaps.

## Completion Decision

FAIL. Fix the residual QC mapping/parity defect, complete the end-to-end DoD-1 fixture matrix, update stale writer documentation, then finalize `review.md`, conductor status, ledger commit, and operator evidence.