# Track Completion Audit — 0080-UniquePstOutlookQc

## Verdict: FAIL

## Scope Reviewed

Reviewed commit `25e13db`, prior r2 findings, complete `spec.md`/`plan.md`, implementation notes, production wiring, tests, docs, deferred entries, and clean working tree.

## Requirement and DoD Matrix

| DoD | Result | Evidence |
|---|---|---|
| 1 | Met | Machine-readable fidelity allowlist and fail-closed unknown classification. |
| 2 | Met | QC JSON/CSV artifacts and `qc_ms` wiring exist. |
| 3 | Met | `unique-pst` levels and standalone `qc-pst` exist. |
| 4 | Met | Folder-tree comparison implemented. |
| 5 | Partial | Attachment hashes compare, but duplicate filenames collapse into one map entry. |
| 6 | Met | Source-differential path and honesty flags are wired. |
| 7 | Met | Existing `structural_digest_pst` is reused. |
| 8 | Met | Deterministic strata and cap are implemented. |
| 9 | Unmet | `unexplained_loss` remains tested through injected metadata, not a corrupted output PST. |
| 10 | Met | Hard findings fail; known gaps do not; existing failures are not lowered. |
| 11 | Unmet | Required full fixture matrix is incomplete. |
| 12 | Partial | BYOB/count handling is improved, but required canonical review/licence evidence is absent. |
| 13 | Partial | Copy/timeout/log/backup handling exists, but `-no repair` behavior is not actually verified. |
| 14 | Met | Attestation is load/record-only and not self-generated. |
| 15 | Met | CC is preserved; BCC is declared/counts as a known gap. |
| 16 | Met | Dated Outlook-retirement documentation exists. |
| 17 | Partial | Deferred rows exist, but the claimed E2E closure exceeds the demonstrated matrix. |
| 18 | Met | Conductor and sequencing rows are updated. |
| 19 | Unmet | Canonical `conductor\0080-UniquePstOutlookQc\review.md` is absent. |
| 20 | Not verifiable | Formatting passed; compilation/clippy/tests were blocked by target lock permissions. |
| 21 | Met | Digest origin and full-coverage checks now prevent silent full-QC claims. |
| 22 | Met | Cloud-attachment blind spot and residual are explicit. |

## Findings

### [P1] Standalone QC can pass with incomplete export metadata

Confidence: High  
Requirement: DoD-3, DoD-4, DoD-6, DoD-21  
Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:2172), [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1804)

Problem: Missing `export_messages.csv` is treated as an empty row set, and truncated row coverage is not checked against `messages_written`. Folder matching also does not require every output folder to be claimed.

Failure scenario: A two-folder output has a report CSV containing only one folder’s row, or no CSV at all. Message count can still match while omitted messages are never content-compared and the report can remain green.

Correction: Require the mandatory CSV, validate per-volume row counts, reject duplicate/missing export indexes, and require all output folder slots to be accounted for.

Verification: Add missing-file, omitted-row, duplicate-index, and extra-unclaimed-folder tests.

Deferrable: No

### [P1] `unexplained_loss` negative coverage remains synthetic

Confidence: High  
Requirement: DoD-9  
Location: [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:1810)

Problem: The new test injects `extra_source_props` into a handcrafted `content_digests.json`. It does not create a corrupted/short-changed output PST that reaches `unexplained_loss` through the production comparison path.

Correction: Add a generated PST negative that observes an actually uncontracted property/difference without probe hooks or hand-authored digest metadata.

Verification: Require the test to mutate/generated-output-only data and assert `unexplained_loss` plus exit failure.

Deferrable: No

### [P1] ScanPST `-no repair` support is still not behaviorally verified

Confidence: High  
Requirement: DoD-13; locked rule 2  
Location: [qc_external.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/qc_external.rs:325)

Problem: A sibling `.accepts-no-repair` file or environment variable is accepted as proof, while the help probe only searches for the phrase. Neither proves that the executable honors the exact arguments and will not enter the repairing legacy path.

Failure scenario: An incompatible executable is accompanied by the marker or help text, receives `-no repair`, silently repairs the temp copy, and is reported based on its log.

Correction: Use a genuinely verified build/behavior probe; otherwise always skip. Record the unresolved operator residual honestly.

Deferrable: No

### [P1] Canonical completion review and operator evidence are missing

Confidence: High  
Requirement: DoD-19 and completion governance  
Location: `conductor\0080-UniquePstOutlookQc\review.md` (absent); [conductor.md](/C:/dev/Dedupe/conductor/conductor.md:181)

Problem: Only `review.codex.md` and `review.codex.round2.md` exist. The canonical review required by the track is absent, and the conductor/sequencing status remains `In Progress`.

Correction: After fixes and gates, create the canonical review containing the explicit CI scanpst absence reason, exact gate results, findings/dispositions, and final decision.

Deferrable: No

### [P2] Duplicate attachment filenames can produce a false green

Confidence: High  
Requirement: DoD-5  
Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1437)

Problem: Output attachments are indexed as `BTreeMap<filename, hash>`. Multiple attachments with the same filename collapse into one entry.

Failure scenario: The source contains two same-named attachments with identical payloads and the output contains one. Both source entries find the same output hash and QC reports no loss.

Correction: Use filename-keyed multisets and consume each output attachment exactly once, matching filename, size, and payload hash.

Deferrable: No

### [P2] Full fixture-matrix proof remains incomplete

Confidence: High  
Requirement: DoD-11 and rolled-in E2E closure  
Location: [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:2035)

Problem: The added multi-volume test is structure-only; the non-ASCII and oversized-subject tests manually construct clean-room digests. There is still no complete production-path full-QC matrix covering multi-source prefixes, zero-byte/XBLOCK attachments, embedded messages, degraded messages, and zero-winner output together.

Correction: Add full `unique-pst` production-path tests for every required fixture dimension and combined cases, then reconcile the deferred E2E claim.

Deferrable: No

## Completeness Sweep

No new blocking production TODO/stub markers were found. The probe hook remains a diagnostic/test escape hatch; its continued use for the DoD-9 unexplained-loss proof is covered above.

## Verification Evidence

Observed:

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- Targeted QC test: failed before compilation because `target\debug\.cargo-build-lock` could not be opened (`Access is denied`).
- `cargo clippy --workspace --all-targets -- -D warnings`: same target-lock failure.
- `cargo test --workspace`: same target-lock failure.
- `ledgerful ledger status --compact`: unavailable, `unable to open database file`.
- `ledgerful scan --impact`: unavailable because it could not write its report; cached impact was stale.
- Real ScanPST/operator smoke: absent; the residual is documented in `docs\deferred.md`.

## Completion Decision

FAIL. The r2 fixes are partially validated by inspection, but the standalone metadata false-success path, real `unexplained_loss` negative, ScanPST safety verification, incomplete fixture matrix, and missing canonical review remain blocking.