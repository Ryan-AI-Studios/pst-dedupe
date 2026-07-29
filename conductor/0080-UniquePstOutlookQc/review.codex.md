# Track Completion Audit — 0080-UniquePstOutlookQc

## Verdict: FAIL

## Scope Reviewed

Reviewed `ce9cfc8..b1e625e`, working tree, full `spec.md`, `plan.md`, implementation notes, production wiring, tests, docs, and external-validator code.

No files were modified. `ledgerful` was unavailable (`unable to open database file`). Cached impact data was stale. `cargo fmt --all --check` passed; clippy and workspace tests were not independently observed.

## Requirement and DoD Matrix

| DoD | Result | Evidence / gap |
|---|---|---|
| 1 | Partial | Contract exists, but production comparisons do not classify all differences safely. |
| 2 | Partial | Artifacts are wired, but write errors are discarded and zero-volume exports skip QC. |
| 3 | Partial | CLI surfaces exist; standalone QC has no-MID/path/CSV defects. |
| 4 | Unmet | Folder presence is checked, not per-folder counts; ancestor matching can false-pass. |
| 5 | Partial | Attachment hashes are read, but read failures and broad explanations undermine coverage. |
| 6 | Partial | Source comparison exists, but source errors are fail-open and matching can misassociate. |
| 7 | Met | `structural_digest_pst` is promoted and reused. |
| 8 | Partial | Deterministic sampling exists; several strata are only first-item representatives. |
| 9 | Partial | Corruption tests cover defects; unexplained loss uses a diagnostic hook, not a production path. |
| 10 | Partial | Counter logic is correct in isolation, but real findings can be suppressed. |
| 11 | Unmet | Full fixture-matrix proof was not demonstrated; only limited fixture coverage is present. |
| 12 | Partial | BYOB/counts-only sidecar exists, but only the first volume is checked. |
| 13 | Unmet | Build/token verification is guessed for `Office16`; missing logs can report `Ok`. |
| 14 | Met | Human attestation is loadable and never auto-generated. |
| 15 | Met | CC is written; BCC is declared and counted as a known gap. |
| 16 | Met | Dated Outlook-retirement documentation exists. |
| 17 | Partial | Deferred entries exist, but several closures overstate unverified behavior. |
| 18 | Partial | Registry remains `In Progress`; sequencing is updated. |
| 19 | Unmet | Required canonical `review.md` and operator evidence are absent. |
| 20 | Partial | Formatting passed; clippy/tests were not observed. |
| 21 | Partial | Source digest persistence exists, but standalone no-MID handling is broken. |
| 22 | Met | Cloud-attachment blind spot and residual are explicitly documented. |

## Findings

### [P1] Folder QC does not verify folder counts

Confidence: High  
Requirement: DoD-4  
Location: `crates/pst-dedup-cli/src/unique_pst_qc.rs:544-559,1293-1328`  
Problem: QC compares only the presence of expected folder leaves and global message count. It does not compare per-folder counts, and `contains("/leaf/")` allows ancestor matches.  
Failure scenario: Messages redistributed between Inbox and Sent Items still pass if both folders exist and the volume total matches.  
Correction: Persist per-folder counts and compare exact normalized paths/counts; reject ancestor-only matches.  
Verification: Add a synthetic same-leaves/different-counts fixture and an ancestor-vs-leaf fixture.

### [P1] Broad degradation flags suppress unrelated fidelity defects

Confidence: High  
Requirement: DoD-5, DoD-6, DoD-10  
Location: `unique_pst_qc.rs:1096-1138,1168-1223`; candidate setup at `unique_pst_cmd.rs:1914-1930,2187-2193`  
Problem: Any degraded reason explains all digest/body differences. Any attachment failure on a message explains every missing attachment, regardless of the affected attachment.  
Failure scenario: A degraded message loses CC, body, or a preserved attachment and QC reports no hard finding.  
Correction: Carry field- and attachment-specific explanation identities; only classify the exact documented loss as `explained`.  
Verification: Mutate an unrelated preserved field on degraded/soft-fail messages and require `defect`.

### [P1] Source and attachment read errors fail open

Confidence: High  
Requirement: DoD-5, DoD-6  
Location: `unique_pst_qc.rs:911-940`; `export_oracle.rs:562-574`  
Problem: Existing-path parse/read failures are labeled `skipped_source_unavailable`/`Explained`. Attachment-list errors are treated as an empty attachment list.  
Failure scenario: A corrupt or unreadable source can produce a green QC report with no source comparison.  
Correction: Distinguish missing source paths from corrupt/read-failed sources; propagate attachment enumeration failures as explicit findings.  
Verification: Test an existing malformed source and an attachment-list failure.

### [P1] Standalone `qc-pst` can validate the wrong file or false-fail no-MID messages

Confidence: High  
Requirement: DoD-3, DoD-6, DoD-21  
Location: `unique_pst_qc.rs:1418-1437,1475-1535,1538-1587`  
Problem: When `summary.json` exists, the positional `out.pst` is ignored. The CSV parser is a raw `split(',')`, and standalone candidates never load subjects, so messages without Message-IDs cannot match.  
Failure scenario: QC silently checks the summary’s old volume path instead of the supplied output, or reports every no-MID message missing.  
Correction: Honor the supplied output path, use a real CSV parser, and hydrate no-MID candidates from persisted digest/export metadata.  
Verification: Test moved outputs, quoted Windows paths, and no-MID messages.

### [P1] QC artifacts can disappear without failing the export

Confidence: High  
Requirement: DoD-2, DoD-3, DoD-19  
Location: `unique_pst_cmd.rs:2437`; `unique_pst_qc.rs:849-865`  
Problem: QC is skipped when `volumes.is_empty()`, and all QC artifact write errors are ignored.  
Failure scenario: A zero-winner export or unwritable report directory produces no QC artifact while the export can remain green.  
Correction: Run zero-volume QC when enabled; propagate artifact-write failures into `report_ok`/verification failure.  
Verification: Add zero-winner and unwritable-report tests.

### [P2] Scanpst safety and coverage do not meet the locked contract

Confidence: High  
Requirement: DoD-12, DoD-13  
Location: `qc_external.rs:278-325,413-434`; `unique_pst_qc.rs:718-800`  
Problem: `Office16` is treated as a verified minimum build without reading the installed version; missing logs can still produce `ExternalStatus::Ok`; only the first volume is passed to external validators.  
Failure scenario: An unverified scanpst build runs, or a multi-volume export receives only first-volume corroboration.  
Correction: Skip unless file-version/argument behavior is verified; require a parseable log; run external checks for every volume and aggregate results.  
Verification: Add unknown-build, empty-log, and multi-volume stub tests.

### [P2] The unexplained-loss negative test is not a production-path test

Confidence: High  
Requirement: DoD-9  
Location: `unique_pst_qc.rs:186-188,701-715`; `tests/unique_pst_qc_0080.rs:246-289`  
Problem: The `unexplained_loss` test injects `probe_unexplained_property`; no corrupted or short-changed PST exercises an actual production comparison path that emits this class.  
Correction: Build a real source/output mismatch involving an unmapped property, or explicitly test the production classifier through a real observed difference.  
Verification: Remove reliance on the probe-only hook for DoD-9.

## Completeness Sweep

No blocking TODO/stub strings were found in the main QC implementation. However, the corruption helpers are public production functions (`unique_pst_qc.rs:1594-1618`) despite being test-only utilities; they should be test-scoped or otherwise removed from the public runtime surface.

## Verification Evidence

- `cargo fmt --all --check`: passed.
- `git diff --check`: failed on trailing whitespace in `implementation-notes.md`.
- `cargo clippy --workspace --all-targets -- -D warnings`: not observed.
- `cargo test --workspace`: not observed.
- `ledgerful ledger status --compact`: unavailable due database error.
- No real Outlook/scanpst operator evidence; implementation notes explicitly say scanpst is absent in CI.

## Deferred Candidates

None of the findings qualify as deferrable P3s. The external-reader version matrix and cloud-attachment named-property support are documented residuals, but the scanpst verification behavior itself violates a locked DoD and is not deferrable.

## Completion Decision

FAIL. Fix the P1 findings, repair the P2 contract/test gaps, rerun the full verification gate, and produce the canonical `conductor\0080-UniquePstOutlookQc\review.md` before marking the track completed.