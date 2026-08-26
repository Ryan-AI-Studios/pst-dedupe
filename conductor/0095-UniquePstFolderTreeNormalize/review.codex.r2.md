# Track Completion Audit — 0095-UniquePstFolderTreeNormalize

## Verdict: FAIL

## Scope Reviewed

`track/0095-UniquePstFolderTreeNormalize` working tree versus `main` at `850dce2`, including staged, unstaged, and untracked changes. Read-only review of spec, plan, prior review, implementation, tests, docs, deferred ledger, and governance.

## Requirement and DoD Matrix

| Requirement | Status | Evidence |
|---|---|---|
| Phase 0 triage | Met | `phase0-triage.md` classifies prefix race and related asymmetries. |
| Alias stripping | Met | Leading consecutive sentinel stripping implemented and unit-tested. |
| D-0070 pre-seeding | Met for `unique-pst` | Winner source paths passed through `known_source_paths`; message-one prefix test exists. |
| Lazy preserve `Unique Mail` | Met | Residual allocation is lazy; empty-preserve tests verify absence. |
| Flat isolation | Met | Flat layout remains eager and path-independent. |
| Residual QC mapping | Met | `folder_path_qc_expected_key` mirrors writer residual routing. |
| Deleted Items QC | Met | Message-bearing Deleted Items remains claimable. |
| DoD-1 end-to-end matrix | Partial | New writer→QC fixture exists, but malformed residual variants are not all exercised end-to-end. |
| DoD-2 documentation | Met | Tree contract and writer fidelity docs updated. |
| DoD-3 D-0070 closure | Met in implementation; ledger not independently verifiable | Deferred entry is closed; Ledgerful status unavailable under read-only restrictions. |
| DoD-4 gates | Reported passed; not independently runnable | Cargo commands were blocked by read-only access to `target\debug\.cargo-lock`. |
| DoD-5 finalization | Unmet | No canonical `review.md`; track and conductor remain `Ready`; plan Phase 4 remains unchecked. |

## Findings

### [P1] Track completion governance is not finalized

Confidence: High  
Requirement: DoD-5  
Location: `conductor/0095-UniquePstFolderTreeNormalize/plan.md:48`, `spec.md:218`, `conductor/conductor.md:226`

Problem: The track still states `Ready`, Phase 4 is unchecked, and no canonical `review.md` exists. Ledger commit status could not be verified because Ledgerful cannot access its database under the enforced read-only filesystem.

Evidence: `Get-ChildItem` found only prior/internal review files; `conductor.md` still records 0095 as `Ready`.

Failure scenario: The implementation may be technically complete, but the track cannot be marked completed with the required canonical audit and provenance record.

Correction: After engineering blockers are resolved, write the canonical `review.md`, mark the track `Completed`, and verify/record the Ledgerful BUGFIX commit and operator re-smoke evidence.

Verification: Re-run `ledgerful ledger status --compact` and confirm the governance files and ledger entry.

Deferrable: No

### [P2] Residual edge cases remain only partially end-to-end tested

Confidence: High  
Requirement: DoD-1 residual matrix and r1 P2 correction  
Location: `crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:2517`

Problem: The new writer→QC fixture covers a `None` folder path, but not empty-string, alias-only, `..`, or over-depth paths through actual PST writing and QC. Those cases are covered only by synthetic QC digests or normalization/parser unit tests.

Evidence:

- End-to-end fixture uses `source_folder_path = None` at `unique_pst_qc_0080.rs:2543-2545`.
- Synthetic residual coverage is at `unique_pst_qc.rs:3317-3345`.
- Parser/expected-key unit coverage is in `production.rs:6028-6041`.

Failure scenario: A divergence between writer routing and QC expected-key handling for alias-only, traversal, or over-depth paths could regress while the current end-to-end test remains green.

Correction: Extend the writer→QC fixture with explicit empty, alias-only, `..`, and over-depth messages, then assert the real PST’s residual `Unique Mail` count and passing `folder_tree_match`.

Verification: Run `cargo test -p pst-dedup-cli --test unique_pst_qc_0080` and the relevant writer tests.

Deferrable: No

## Completeness Sweep

No new blocking placeholders, stubs, fake success paths, or production `unwrap`/`expect` usage were found in the touched implementation. The root `agy-review.md` remains untracked as the plan directs.

## Wiring and Regression Review

The core path is wired:

`unique-pst winners → known source pre-seed → streaming writer → PST reader structural digest → normalized QC expected/output keys → folder_tree_match`

The r1 residual QC defect, Deleted Items asymmetry, lazy residual allocation, alias stripping, and stale writer documentation are fixed.

## Verification Evidence

Observed:

- `cargo fmt --all --check`: passed.
- Cargo tests and clippy could not execute because the read-only environment denied access to `C:\dev\Dedupe\target\debug\.cargo-lock`.
- `ledgerful ledger status --compact`: failed with `unable to open database file`.
- `ledgerful scan --impact`: inspected the tree but could not write its report under read-only restrictions.
- AI-Brains recall/preflight unavailable because the vault key was missing.

Reported by orchestrator/user:

- clippy `-D warnings`: passed.
- `cargo test -p pst-writer`: passed.
- `unique_pst_qc_0080`: 59 passed.
- `unique_pst`: 31 passed.
- formatting: passed.

## Deferred Candidates

None. The remaining findings are P1/P2 and are not deferrable.

## Completion Decision

FAIL. The implementation fixes the r1 P1 and stale-documentation findings, but the residual matrix needs complete production-path coverage and the required DoD-5 governance finalization remains outstanding.