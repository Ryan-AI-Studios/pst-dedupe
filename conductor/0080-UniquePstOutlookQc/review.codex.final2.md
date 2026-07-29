# Track Completion Audit — 0080-UniquePstOutlookQc

## Verdict: FAIL

## Scope Reviewed

Commit `56e4021`, clean working tree, full `spec.md`/`plan.md`, implementation notes, production wiring, tests, docs, deferred entries, and prior review findings.

## Requirement and DoD Matrix

| DoD | Result | Evidence |
|---|---|---|
| 1 | Met | Fidelity allowlist exists. |
| 2 | Met | QC JSON/CSV artifacts and timing wiring exist. |
| 3 | Met | `unique-pst` levels and `qc-pst` exist. |
| 4 | Met | Folder tree/count comparison exists. |
| 5 | Partial | Attachment multiset comparison exists, but policy paths can skip unexpected output attachments. |
| 6 | Partial | Source comparison exists; standalone metadata and clean-room joins remain weak. |
| 7 | Met | Existing structural digest is reused. |
| 8 | Met | Deterministic strata and cap are implemented. |
| 9 | Unmet | `unexplained_loss` remains tested through crafted digest metadata, not a byte-edited PST. |
| 10 | Met | Hard findings fail; known gaps do not; exits are not lowered. |
| 11 | Unmet | Full production-path fixture matrix is incomplete. |
| 12 | Partial | BYOB/count behavior exists; licence evidence belongs in the missing canonical review. |
| 13 | Partial | Stub paths are covered, but production `-no repair` behavior is not verified. |
| 14 | Met | Attestation is load-only. |
| 15 | Met | CC is preserved; BCC is counted as `known_gap`. |
| 16 | Met | Client-retirement documentation exists. |
| 17 | Partial | Deferred claims overstate demonstrated E2E closure. |
| 18 | Unmet | Track remains `In Progress`. |
| 19 | Unmet | Canonical `review.md` is absent. |
| 20 | Not verifiable | Formatting passed; Cargo gates were blocked by target lock permissions. |
| 21 | Partial | Digest coverage is guarded, but explanation metadata is not persisted. |
| 22 | Met | Cloud-attachment blind spot and residual are explicit. |

## Findings

### [P1] Standalone QC can still false-green manipulated export metadata

Confidence: High  
Requirement: DoD-3, DoD-4, DoD-6, DoD-21  
Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1205), [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1259)

Problem: Validation checks missing/empty CSVs, duplicate global indexes, and per-volume row counts, but does not require every row to reference a declared volume or verify exact index coverage. Malformed summary fields also default to zero/empty values.

Failure scenario: A row is replaced by an unknown-volume or wrong-source row while known-volume counts remain equal. That row is never compared in the volume loop, allowing a green report.

Correction: Strictly validate summary fields, declared volume membership, per-volume index coverage, and unclaimed rows.

### [P1] Required `unexplained_loss` byte-edit negative is still missing

Confidence: High  
Requirement: DoD-9  
Location: [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:1808), [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:2518)

Problem: The test injects `extra_source_props` into handcrafted `content_digests.json`. It does not generate a changed PST that reaches `unexplained_loss` through the real comparison path.

Correction: Add the required writer-generated, byte-edited negative without probe hooks or hand-authored unknown-property metadata.

### [P1] Contract-before-default-on fixture proof remains incomplete

Confidence: High  
Requirement: DoD-11  
Location: [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:2033), [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:2118), [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:2195)

Problem: Multi-volume coverage is structure-only; non-ASCII and oversized-subject coverage uses clean-room handcrafted digests. There is no complete production-path full-QC matrix covering the required combined cases, including embedded, XBLOCK-sized, degraded/soft-fail, and multi-volume output.

Correction: Add the complete production-path matrix and prove zero hard findings before retaining default `sample`.

### [P1] Production ScanPST `-no repair` behavior is not actually verified

Confidence: High  
Requirement: DoD-13; locked rule 2  
Location: [qc_external.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/qc_external.rs:317), [qc_external.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/qc_external.rs:422)

Problem: Help text is accepted as proof for a real executable. Only test stubs provide a behavioral `NO_REPAIR_MODE` marker. A binary can document the flag without honoring it.

Correction: Require a real behavioral probe or skip the production validator; retain the stub-only proof for CI.

### [P2] Clean-room digests omit fidelity-explanation metadata

Confidence: High  
Requirement: DoD-5, DoD-6, DoD-21  
Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:517), [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:2143)

Problem: `content_digests.json` does not persist body fidelity flags, CRC state, or attachment-specific ledger filenames. Standalone QC reconstructs candidates with those flags empty.

Failure scenario: A live export correctly explains a soft attachment/body loss, but later source-gone `qc-pst` reclassifies it as `unexplained_loss` or `defect`.

Correction: Persist the explanation metadata or load the exact attachment ledger and fidelity flags during clean-room QC.

### [P2] External-reader aggregate can report `Ok` when a volume was skipped

Confidence: High  
Requirement: DoD-12  
Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1118), [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1176)

Problem: Status ranking lets `Ok` dominate `Skipped`. A multi-volume run can therefore report aggregate `Ok` despite one volume not being checked.

Correction: Preserve per-volume status or make any skipped volume yield aggregate `Skipped` unless a harder status applies.

## Completeness Sweep

No new blocking production TODO/stub/no-op markers were found. Test-only stubs are appropriately confined. The documented `D-0080-unexplained-byte-edit` residual conflicts with the explicit DoD-9 requirement and cannot be accepted as a completion deferral.

## Wiring and Regression Review

The main path is wired:

`unique-pst/qc-pst → metadata load → output structural digest → source/clean-room comparison → findings/artifacts → hard_fail → existing verification/exit contract`

The primary remaining risks are incomplete metadata binding, insufficient clean-room provenance, incomplete fixture proof, and ScanPST safety verification.

## Verification Evidence

Observed:

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- `cargo metadata --no-deps`: passed.
- Targeted QC tests: blocked before compilation by `Access is denied` opening `target\debug\.cargo-build-lock`.
- `cargo clippy --workspace --all-targets -- -D warnings`: same blocker.
- `cargo test --workspace`: same blocker.
- `ledgerful ledger status --compact`: unavailable, database could not be opened.
- `ledgerful scan --impact`: unavailable, report could not be written; cached impact was stale and referenced `25e13db`.
- Real ScanPST/operator smoke: not observed.

## Deferred Candidates

None. The findings are DoD or correctness issues, not qualifying P3 deferrals.

## Completion Decision

FAIL. Fix the six findings, run the full Cargo gate in a writable environment, complete the fixture matrix, and produce the canonical completion review.