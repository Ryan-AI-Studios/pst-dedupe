# Track Completion Audit - 0082-RecipientTableFidelity

## Verdict: FAIL

## Scope Reviewed

Read all of `spec.md` and `plan.md`, then audited the working-tree implementation, tests, docs, deferred rows, board status, and available Ledgerful evidence. No files or Git state were modified.

## Requirement and DoD Matrix

| Area | Result | Evidence |
|---|---|---|
| Reader: recipient TC `0x12`, flags, no Display* invention | Partial | Implementation is present; required missing-table integration test is absent. |
| Writer: template `0x692`, 14 MUST columns, optional SMTP, empty per-message TC | Pass by source audit | [`production.rs`](C:/dev/Dedupe/crates/pst-writer/src/production.rs:4178) and round-trip tests. |
| Pipeline and identity cascade | Partial | SMTP/EX/display and empty-key fallback are implemented; typed X.500 telemetry is incomplete. |
| BCC write-off/hash-on and suppression ledger | Pass by source audit | Flag, filtering, hash inclusion, CSV and summary counters are wired. |
| QC and clean-room BCC policy | Pass by source audit | QC filters BCC correctly and reads `summary.export.include_bcc_recipients`. |
| Retryable classification | Fail | Writer I/O failures are converted to generic export errors and become non-retryable. |
| Contract, docs, deferred dispositions | Pass by source audit | `recipient_table` is `Preserved`; docs and deferred rows are updated. |
| Dependencies | Pass | No `Cargo.toml` or `Cargo.lock` diff. |
| Completion governance | Fail | Review file absent; board remains `Ready`; Phase 6 remains unchecked. |

### DoD status

| DoD | Status |
|---|---|
| 1 Reader | Partial |
| 2 Writer | Pass by source audit |
| 3 BCC write policy | Pass by source audit |
| 4 Pipeline | Pass by source audit |
| 5 Identity | Partial |
| 6 Contract | Pass |
| 7 QC | Pass by source audit |
| 8 Retryable | Partial/Fail |
| 9 BCC ledger | Pass |
| 10 Zero-recipient anomaly | Pass by source audit |
| 11 Docs/deferred | Pass by source audit |
| 12 Dependencies | Pass |
| 13 Gates | Reported PASS by orchestrator; not independently run |
| 14 Recorded completion | Fail/not verifiable |

## Findings

[P1] Track completion evidence is not recorded

Confidence: High

Requirement: DoD-14; plan Phase 6.

Location: [`plan.md`](C:/dev/Dedupe/conductor/0082-RecipientTableFidelity/plan.md:91), [`ROADMAP.md`](C:/dev/Dedupe/conductor/ROADMAP.md:356), [`conductor.md`](C:/dev/Dedupe/conductor/conductor.md:193)

Problem: `review.md` is absent, Phase 6 remains unchecked, and both roadmap/conductor entries still report `0082` as `Ready`. Ledger status could not be verified because the read-only environment rejected the Ledgerful database/report writes.

Evidence: `ledgerful ledger status --compact` failed with `unable to open database file`; `ledgerful scan --impact` failed because it could not write `latest-scan.json`.

Failure scenario: The implementation cannot be declared complete with the required evidence and provenance record.

Correction: After code fixes, record the canonical review, update both board entries to `Completed`, and verify/commit the Ledgerful transaction.

Verification: Re-run Ledgerful status/verify and confirm `review.md`, board status, and committed transaction.

Deferrable: No

[P2] Writer I/O failures are incorrectly marked non-retryable

Confidence: High

Requirement: Spec §2.7.4 and DoD-8: clear transient write/disk I/O must yield `retryable: true`.

Location: [`unique_pst_cmd.rs`](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:2324), [`unique_pst_cmd.rs`](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:2731), [`export_outcome.rs`](C:/dev/Dedupe/crates/pst-dedup-cli/src/export_outcome.rs:278)

Problem: `WriterError::Io` is converted to an untyped `"export"` summary error. Classification adds `COUNT_MISMATCH`, which `summary_is_retryable` treats as permanent. The helper’s `write_io` test path exists, but production never supplies that classification.

Failure scenario: Disk-full or transient I/O during volume writing produces `retryable: false`, preventing safe automation retry.

Correction: Preserve typed writer-error classification through summary generation; mark only confirmed transient I/O/cancel cases retryable, while keeping layout, integrity, count, and fidelity failures false.

Verification: Add an orchestration-level injected writer-I/O test asserting `retryable: true`, plus permanent writer-error tests asserting false.

Deferrable: No

[P2] Typed EX/X.500 handling is incomplete for telemetry and non-`/O=` forms

Confidence: High

Requirement: Spec §2.5 rule 4, §2.7.3, and DoD-5.

Location: [`recipient.rs`](C:/dev/Dedupe/crates/pst-reader/src/messaging/recipient.rs:117), [`grouping.rs`](C:/dev/Dedupe/crates/dedup-engine/src/grouping.rs:445), [`scan.rs`](C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:902)

Problem: EX detection recognizes only address type `EX` or an email containing `/O=`. `identity_is_x500()` likewise only checks `/O=`. A typed EX row with a LegacyExchangeDN such as `/CN=Recipients/...` can hash using the EX key but is omitted from X.500 telemetry; typed X500 forms can fall through to display identity.

Correction: Recognize the documented EX/X.500/LegacyExchangeDN forms from structured fields, preserving the full normalized key. Add typed EX and non-`/O=` test cases.

Verification: Assert identity and `x500_recipient_items` behavior for typed EX/X.500 rows without `/O=`.

Deferrable: No

[P2] Required missing-table reader test is absent

Confidence: High

Requirement: DoD-1 and Q4: missing/unreadable tables return an empty vector while Display* remains unchanged and no rows are invented.

Location: [`recipient.rs`](C:/dev/Dedupe/crates/pst-reader/src/messaging/recipient.rs:138), [`writer_fidelity.rs`](C:/dev/Dedupe/crates/pst-writer/tests/writer_fidelity.rs:1065)

Problem: The reader code handles missing/corrupt tables, but the tests only cover writer-produced empty tables and a unit-level empty vector. No test opens a message with Display* values and an absent/unreadable recipient subnode.

Correction: Add a synthetic missing/corrupt-table fixture and assert empty structured recipients, preserved Display* values, and no invented rows.

Verification: Run the targeted reader/writer fixture test.

Deferrable: No

[P2] Generated artifacts remain in the repository tree

Confidence: High

Requirement: Repository hygiene; no temporary output outside designated output paths.

Location: `crates/pst-dedup-cli/-no.bak`, `fixtures/keep_set_summary.json`

Problem: Both are untracked generated artifacts. The summary contains local source paths and is not referenced by the implementation or tests.

Correction: Confirm ownership, then remove or relocate generated output before completion. No cleanup was performed because this review is read-only.

Verification: Confirm only intended implementation/test files remain untracked.

Deferrable: No

## Completeness Sweep

The following were confirmed by source audit:

- Template `0x692` is emitted with the 14 MUST columns plus the additive SMTP column.
- Every written message receives a recipient TC, including zero-row tables.
- BCC defaults off; opt-in wiring reaches writer options, GUI defaults, summary, and QC.
- BCC participates in identity hashing even when omitted from the deliverable.
- Empty structured rows fall back to display hashing without inventing reader rows.
- `recipient_table` is marked `Preserved`; BCC remains explicitly policy-dropped.
- No new exit codes or `export_risk` values were added.
- Deferred documentation is honest about the remaining `extract-pst` Display-only path and out-of-scope work.

`git diff --check` also reports trailing Markdown whitespace in the new identity-cascade list; this is nonfunctional but should be cleaned with the other handoff hygiene.

## Wiring and Regression Review

The main data flow is connected:

`pst-reader Recipient TC → scan/materialize → CanonicalMessage → strong hash and writer → QC`

The BCC filter is honored in both production QC and clean-room QC. The main regression risk found is the retryability classification gap above.

## Verification Evidence

Reported by the orchestrator:

- `cargo fmt --all --check` PASS
- `cargo clippy --workspace --all-targets -- -D warnings` PASS
- `cargo test --workspace` PASS
- `cargo deny check` PASS
- Internal review: PASS with deferred P3

Not independently executed here because the workspace is read-only. `ledgerful verify` was not verifiable for the same environment restriction.

## Deferred Candidates

No new P3 is recommended for `deferred.md`. The identified issues are required completion fixes, not difficult non-blocking residuals.

Existing out-of-scope residuals—Mode A promotion, named properties/cloud attachments, deterministic keys, unique-EML ledger parity, and operator scanpst evidence—remain appropriately deferred.

## Completion Decision

FAIL. The core recipient-table implementation is substantial, but completion requires fixing retryable I/O classification, completing typed EX/X.500 handling, adding the missing-table test, cleaning generated artifacts, and recording the required review/board/Ledgerful evidence.