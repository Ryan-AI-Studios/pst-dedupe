# Track Completion Audit — 0095-UniquePstFolderTreeNormalize

## Verdict: FAIL

## Scope Reviewed

Current branch `track/0095-UniquePstFolderTreeNormalize`, including staged, unstaged, and untracked changes; track spec/plan/reviews, writer/QC implementation, tests, docs, and governance.

## Requirement and DoD Matrix

| Requirement | Status | Evidence |
|---|---|---|
| Prior r1 findings | Met | Residual QC mapping, writer→QC matrix, and docs are present. |
| Prior r2 residual matrix | Met | Matrix covers None, empty, alias-only, `..`, and over-depth; asserts `Unique Mail == 5` and `folder_tree_match`. |
| DoD-1 | Met by inspection/reported tests | Real PST writer→reader→QC path is wired. |
| DoD-2 | Met | Tree contract documented in `docs/unique-pst-export.md`. |
| DoD-3 | Met by implementation/docs | `known_source_paths` pre-seeding and D-0070 closure are present. |
| DoD-4 | **Unmet** | Current `cargo fmt --all --check` fails. |
| DoD-5 | Partially verifiable | `review.md`, Completed statuses, and checked Phase 4 are present; Ledgerful status remains unavailable read-only. |

## Findings

### [P2] Required formatting gate currently fails

Confidence: High  
Requirement: DoD-4  
Location: [unique_pst_qc_0080.rs:2559](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:2559)

Problem: `cargo fmt --all --check` currently exits 1 and requests multiline formatting for the `over_depth` expression.

Evidence: Directly observed during this review. `git diff --check` also reports trailing whitespace at [spec.md:28](/C:/dev/Dedupe/conductor/0095-UniquePstFolderTreeNormalize/spec.md:28).

Correction: Run `cargo fmt --all` and remove the reported trailing whitespace, then rerun the formatting gate.

Verification: `cargo fmt --all --check` must pass.

Deferrable: No

## Completeness Sweep

No new blocking placeholders, stubs, fake-success paths, or production error-handling regressions were found. Test-only `expect`/`panic` usage is confined to fixtures and assertions.

## Wiring and Regression Review

The prior functional findings are closed:

`unique-pst winners → known source pre-seed → writer → PST reader digest → normalized QC keys → folder_tree_match`

Alias stripping, lazy residual allocation, Deleted Items matching, prefix pre-seeding, and all residual variants are wired and covered by the current matrix.

## Verification Evidence

Observed:

- `cargo fmt --all --check`: **failed**.
- `git diff --check`: **failed** on trailing whitespace.
- `ledgerful ledger status --compact`: unavailable: `unable to open database file`.
- `ledgerful scan --impact`: completed with read-only report-write warning.

Reported by the user/current track artifacts:

- clippy passed.
- `cargo test -p pst-writer` passed.
- `unique_pst_qc_0080` passed, 59 tests.
- `unique_pst` passed, 31 tests.
- formatting passed — contradicted by the current direct check.

## Deferred Candidates

None. The formatting issue is an explicit DoD gate and is not deferrable.

## Completion Decision

**FAIL.** Prior r2 functional and governance findings are closed, but the current checkout does not pass the required formatting gate.