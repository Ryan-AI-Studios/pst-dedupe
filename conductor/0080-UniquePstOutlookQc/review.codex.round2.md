# Track Completion Audit — 0080-UniquePstOutlookQc

## Verdict: FAIL

## Scope Reviewed

Reviewed `ce9cfc8..98d1279`, clean working tree, `spec.md`, `plan.md`, implementation notes, production wiring, tests, docs, deferred entries, and prior Codex FAIL findings.

## Requirement and DoD Matrix

| DoD | Result | Evidence |
|---|---|---|
| 1 | Met | Machine-readable allowlist contract exists. |
| 2 | Met | QC JSON/CSV artifacts and `qc_ms` wiring exist. |
| 3 | Met | `unique-pst --qc-level` and standalone `qc-pst` exist. |
| 4 | Met | Per-folder counts and path matching added. |
| 5 | Partial | Attachment hashes are read, but explanations remain message-wide. |
| 6 | Partial | Source comparison exists; identity and clean-room coverage gaps remain. |
| 7 | Met | Reuses promoted `structural_digest_pst`. |
| 8 | Met | Deterministic risk strata and cap tests exist. |
| 9 | Unmet | `unexplained_loss` is tested through a probe hook, not a corrupted output PST. |
| 10 | Partial | Exit wiring is correct, but findings can still be suppressed. |
| 11 | Unmet | Complete fixture-matrix proof is absent and gates could not run. |
| 12 | Partial | BYOB sidecar exists, but accepts relative paths and reports success without counts. |
| 13 | Partial | Copy/timeout/`.bak`/empty-log handling exists; exact `-no repair` verification is not implemented. |
| 14 | Met | Attestation is load-only and not self-generated. |
| 15 | Met | CC is written; BCC is declared/counts as a known gap. |
| 16 | Met | Dated Outlook-retirement documentation exists. |
| 17 | Met | Deferred entries and residuals are documented. |
| 18 | Met | Conductor/sequencing rows are updated. |
| 19 | Unmet | Canonical `conductor\0080-UniquePstOutlookQc\review.md` is absent. |
| 20 | Partial | `cargo fmt` passed; clippy/tests failed before compilation due target lock permissions. |
| 21 | Partial | Source digest persistence exists, but partial digest files are treated as complete. |
| 22 | Met | Cloud-attachment blind spot and residual are explicit. |

## Findings

### [P1] Attachment-level soft-fail flags suppress unrelated attachment defects

Confidence: High  
Requirement: DoD-5, DoD-6, DoD-10  
Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1352)

Problem: `has_ledger_fail` is message-wide. Any missing attachment on that message is classified as explained, without matching the specific attachment to the ledger event.

Failure scenario: Attachment A legitimately soft-fails, while preserved attachment B is accidentally omitted. B’s loss is reported as explained.

Correction: Carry attachment-specific ledger identities and explain only the exact affected attachment. Also check unexpected output attachments.

Verification: Add a two-attachment test where one attachment has a ledger failure and the other is independently missing.

Deferrable: No

### [P1] Clean-room QC treats incomplete digest files as complete

Confidence: High  
Requirement: DoD-21  
Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:544)

Problem: `content_digest_backed` is enabled based only on `origin == "source"`. If a `sample` digest file is later re-run with `--qc-level full`, candidates absent from the persisted digest have no source detail and silently produce no finding.

Failure scenario: A non-sampled message is changed after export; clean-room `qc-pst --qc-level full` claims digest-backed verification but does not compare that message.

Correction: Validate digest coverage and persisted QC level; either restrict comparison to covered entries or emit an explicit unavailable/partial result.

Verification: Export with `sample`, mutate a non-sampled message, then run standalone full QC.

Deferrable: No

### [P1] `unexplained_loss` negative coverage is still probe-only

Confidence: High  
Requirement: DoD-9  
Location: [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:289)

Problem: The unexplained-loss pipeline is exercised through `probe_unexplained_property`; no corrupted or short-changed PST reaches this finding class through a production comparison.

Correction: Add a real output/source mismatch that observes an unmapped property and produces `unexplained_loss` without a diagnostic hook.

Verification: Remove the probe dependency and require a byte-edited/generated PST fixture to fail with `unexplained_loss`.

Deferrable: No

### [P1] scanpst safety verification is incomplete

Confidence: High  
Requirement: DoD-13 and locked rule 2  
Location: [qc_external.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/qc_external.rs:285)

Problem: The implementation trusts a sibling `.version` file or environment variable but does not verify that the installed executable honors the exact `-no repair` token. Additionally, any non-empty log without recognized error text is treated as `Ok`.

Failure scenario: An old or incompatible executable receives `-no repair`, falls back to repair behavior, or emits an unrelated non-empty log and is reported successful.

Correction: Verify argument behavior against the installed build or skip; require a recognized successful log result, otherwise skip/fail.

Verification: Add stubs for unrecognized arguments and unrecognized log content.

Deferrable: No

### [P1] Standalone QC silently drops malformed export rows

Confidence: High  
Requirement: DoD-3, DoD-6, DoD-21  
Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1954)

Problem: CSV records with fewer than nine fields are silently skipped. The summary’s message count can still match the output, while the omitted rows are never content-compared.

Correction: Treat malformed rows, invalid NIDs, and invalid indexes as QC input errors; validate row coverage against summary and digest metadata.

Verification: Corrupt or truncate `export_messages.csv` and require standalone QC to fail rather than pass structurally.

Deferrable: No

### [P2] No-MID messages with duplicate subjects can be misassociated

Confidence: High  
Requirement: DoD-6  
Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1524)

Problem: Output messages without MIDs are keyed by `subj:<subject>`, so duplicate subjects overwrite one another. Fallback matching then compares multiple source messages to one output message.

Correction: Preserve a multimap or bind output candidates using export index/source metadata rather than subject alone.

Verification: Add two no-MID messages with identical subjects and different bodies.

Deferrable: No

### [P2] Independent-reader corroboration can report `Ok` without counts

Confidence: High  
Requirement: DoD-12  
Location: [qc_external.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/qc_external.rs:161)

Problem: An external reader exiting zero is marked `Ok` even when no message or folder counts were parsed. Relative paths are also accepted despite the BYOB contract requiring an absolute path.

Correction: Require an absolute path and parseable counts for `Ok`; otherwise report `Skipped` with a reason.

Verification: Add zero-output and relative-path stubs.

Deferrable: No

### [P2] Required fixture-matrix proof is incomplete

Confidence: High  
Requirement: DoD-11  
Location: [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:620)

Evidence covers the Aspose fixture, simple synthetic PSTs, attachment/CC negatives, and selected unit cases, but not the complete required full-QC matrix: multi-volume split, multi-source prefix, embedded messages, oversized subject, non-ASCII subject, and the combined cases.

Deferrable: No

## Completeness Sweep

No blocking production TODO/stub strings were found. The probe hook remains a test/diagnostic escape hatch rather than a production comparison path, which is covered above.

## Wiring and Regression Review

The main path is wired:

`unique-pst → source reopen/PstHandleCache → structural/content QC → qc_report_v1/qc_findings.csv → verification.ok → existing VERIFY_FAILED`

The major remaining risks are false explanations, incomplete clean-room coverage, malformed standalone metadata, and incomplete scanpst safety verification.

## Verification Evidence

Observed:

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: failed before compilation: `Access is denied` opening `target\debug\.cargo-build-lock`.
- `cargo test --workspace`: failed before compilation for the same reason.
- `ledgerful verify`: failed; fmt passed, clippy/tests failed, and Ledgerful could not write its report/database.
- `ledgerful ledger status --compact`: unavailable: `unable to open database file`.
- `ledgerful scan --impact`: unavailable: report write failure; cached impact was stale.
- Real scanpst/operator smoke: absent; implementation notes record this as CI-unavailable.

## Deferred Candidates

None. All findings are DoD, correctness, or contract issues and are not qualifying P3 deferrals.

## Completion Decision

FAIL. Fix the P1/P2 findings, run the full verification gate in a writable environment, complete the fixture matrix, and produce the canonical `review.md`.