# Track Completion Audit - 0073-ExportAttachmentFailureLedger

## Verdict: FAIL

## Scope Reviewed

Read the complete `spec.md` and `plan.md`, all 14 changed files, writer/CLI/materializer callers, tests, docs, changelog, deferred records, and current working tree. No files or Git state were modified.

## Requirement and DoD Matrix

| Item | Status | Evidence / gap |
|---|---|---|
| DoD-1 Taxonomy | Partial | Writer paths are centralized, but policy/materializer failures do not reach the ledger. |
| DoD-2 Locus events | Partial | Writer DTO is complete; production policy/meta paths are disconnected. |
| DoD-3 Ledger file | Partial | mpsc writer is wired, but initialization failure is swallowed. |
| DoD-4 Invariant | Partial | Normal successful volumes are covered; discarded-volume events can mismatch summary totals. |
| DoD-5 Histogram | Partial | Normal modes work; failure paths can omit or miscount data. |
| DoD-6 Omit ≠ fail | Partial | Low-level writer passes, but CLI `parents_only` emits no omit events/count. |
| DoD-7 Zero-byte success | Met | Writer regression test present. |
| DoD-8 Promote | Met | Accepted `D-0073-promote` residual is documented. |
| DoD-9 Partial honesty | Partial | Column exists, but `off` mode reports zero per-message failures. |
| DoD-10 unique-eml | Met | `D-0073-eml` residual documented. |
| DoD-11 Exit honesty | Met | Existing targeted test and aggregation preserve non-zero exit. |
| DoD-12 CSV injection | Met | Shared neutralization helper and unit tests present. |
| DoD-13 Row cap | Met | Synthetic sink test covers marker and complete histogram. |
| DoD-14 source_id | Partial | Mapping exists, but unknown paths silently map to source `0`. |
| DoD-15 Docs | Met | Export, fidelity, sensitivity, safety, and residual docs updated. |
| DoD-16 Tests | Partial | Focused gates were reported green, but required end-to-end policy/off/error cases are absent; full gate unavailable. |
| DoD-17 Recorded | Unmet/open | No `review.md`; registry remains **In Progress**; Ledgerful commit/status unavailable. |

## Findings

### [P1] `parents_only` omits never reach the accounting sink

Confidence: High  
Requirement: §2.3.4, §3.2, §3.4, DoD-1/2/6  
Location: [pst_materializer.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/pst_materializer.rs:203>), [unique_pst_cmd.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:947>), [production.rs](<C:/dev/Dedupe/crates/pst-writer/src/production.rs:2866>)

Problem: `PstMaterializer::new(ParentsOnly)` returns an empty attachment list before the writer runs. Therefore `record_attach_event(... OmittedByPolicy ...)` is unreachable on the production `unique-pst --family-policy parents_only` path.

Failure scenario: A source message has attachments, but the report shows `attachments_omitted_by_policy: 0` and emits no `ATTACH_OMITTED_BY_POLICY` info rows.

Correction: Preserve attachment metadata while suppressing payloads, or emit policy-omit events from materialization/CLI with full locus information.

Verification: Add an end-to-end `parents_only` fixture asserting info rows, zero failed rows, correct omit count, and unchanged `ok`.

### [P1] Ledger initialization failure is silently downgraded

Confidence: High  
Requirement: §3.4.1/3.4.6, DoD-3/5  
Location: [unique_pst_cmd.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:1043>)

Problem: `AttachLedgerSink::new` errors are logged, then execution continues with no ledger. The run can still report success without the promised default CSV.

Failure scenario: Permission or disk failure prevents `export_attachments.csv` creation; the PST export succeeds and `ok` may remain true, with no report artifact and no failure recorded.

Correction: Fail closed or record the initialization failure as a report error that forces `ok=false`.

Verification: Inject an unwritable report directory and assert non-zero exit, `ok=false`, and an explicit summary error.

### [P1] Events from a discarded volume can violate the histogram/counter invariant

Confidence: High  
Requirement: §3.4.3, DoD-4/5  
Location: [unique_pst_cmd.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:1187>), [unique_pst_cmd.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:1281>)

Problem: The sink receives events during writing, but `attachments_failed_total` is updated only when the volume returns `Ok(report)`. A later hard write failure deletes that volume while its events remain in the global histogram/CSV.

Failure scenario: A soft attachment failure occurs, then disk/layout failure aborts the volume. Summary histogram includes the soft failure, while `export.attachments_failed` excludes it; CSV may point to a deleted volume.

Correction: Make event accounting transactional per volume, or reconcile summary totals and ledger rows consistently when a volume is discarded.

Verification: Force a hard volume failure after a soft attachment event and assert histogram sum equals `attachments_failed` and discarded-volume rows are absent.

### [P2] `--attach-ledger off` makes `attachments_failed_count` falsely zero

Confidence: High  
Requirement: §3.7, DoD-9  
Location: [unique_export_report.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_export_report.rs:511>), [unique_pst_cmd.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:1220>)

Problem: Off mode returns before maintaining per-message failure counts, but `export_messages.csv` still includes `attachments_failed_count`.

Failure scenario: Summary reports failed attachments and exits non-zero, while the affected message row reports `attachments_failed_count=0`.

Correction: Maintain lightweight per-message counts in all modes while suppressing CSV/histogram output in `off`.

Verification: Add an off-mode fixture asserting the message column remains accurate.

### [P2] `ATTACH_META_FAILED` is classified but not exported

Confidence: High  
Requirement: §3.2, Phase 0/1, DoD-1/2  
Location: [pst_materializer.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/pst_materializer.rs:207>), [pst_materializer.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/pst_materializer.rs:260>), [production.rs](<C:/dev/Dedupe/crates/pst-writer/src/production.rs:714>)

Problem: Materialization records `ATTACH_META_FAILED` only in message integrity state. `WriteMessage`/`WriteAttachment` do not carry that fidelity, so the writer emits no corresponding ledger event.

Failure scenario: `list_attachments` fails; the message proceeds with an empty attachment list, but the attach ledger and histogram remain clean.

Correction: Propagate the metadata failure into an attach event or explicit message-level ledger record with locus.

Verification: Add a synthetic materializer/list failure test asserting `ATTACH_META_FAILED` reaches the ledger.

### [P2] Unknown source paths silently join to input 0

Confidence: Medium  
Requirement: §3.3/3.3.1, DoD-14  
Location: [unique_export_report.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_export_report.rs:496>)

Problem: `resolve_source_id` returns `0` when no path matches, which can falsely associate an event with the first input.

Correction: Normalize paths consistently and fail/report an explicit unmapped source rather than assigning a valid source ID.

## Completeness Sweep

No new TODO/FIXME/stub/placeholder/no-op markers were found in the reviewed implementation files. The documented `unique-eml`, promote, GUI, basename, and Vec-event residuals are explicit rather than hidden.

## Wiring and Regression Review

The normal path is wired:

`unique-pst → writer event sink → mpsc CSV thread → ledger finish → summary/export_messages`.

The production `parents_only`, materializer metadata-failure, ledger-init-error, and discarded-volume paths are not honest end to end. CSV neutralization, row cap, zero-byte success, normal histogram accounting, and exit honesty are otherwise implemented coherently.

## Verification Evidence

Observed:

- `cargo fmt --all --check` — passed.
- `git diff --check` — passed.
- CLI binary help exposes both attachment-ledger flags.
- Working tree remains unchanged by review.

Reported by orchestrator, not independently observed:

- `writer_fidelity`: 31 passed.
- `writer_streaming`: 17 passed.
- `unique_pst`: 16 passed.
- Report unit tests: 8 passed.
- Targeted clippy gates: passed.

Blocked in this read-only environment:

- `cargo test -p pst-writer --test writer_fidelity`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

All failed before compilation with:

`failed to open C:\dev\Dedupe\target\debug\.cargo-build-lock — Access is denied`

Ledgerful status/impact commands also failed because its database could not be opened; the cached impact report was read.

## Deferred Candidates

Existing accepted residuals are valid: `D-0073-promote`, `D-0073-eml`, `D-0073-gui`, and `D-0073-basename`.

`D-0073-vec-events` is a legitimate difficult P3 scale residual and is already recorded. No new deferral is proposed.

## Completion Decision

FAIL. The implementation has substantial working functionality, but policy omission accounting, ledger failure honesty, partial-volume invariants, and metadata-failure propagation must be corrected before completion.