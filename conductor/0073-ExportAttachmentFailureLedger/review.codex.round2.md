# Track Completion Audit — 0073-ExportAttachmentFailureLedger

## Verdict: FAIL

### Prior findings

All seven prior findings are fixed in source and targeted tests:

- P1-1 `parents_only` metadata/omit rows: fixed.
- P1-2 ledger initialization failure: fail-closed through `report_write_errors`.
- P1-3 discarded volume accounting: buffered events commit only on successful volumes.
- P2-1 Off-mode message counts: fixed.
- P2-2 `ATTACH_META_FAILED`: propagated through `attach_list_failed`.
- P2-3 unmapped `source_id`: serializes empty, never fake `0`.

### Finding

[P2] Per-attachment probe failures are misclassified and can be double-counted

Confidence: High  
Requirement: DoD-1, DoD-2, DoD-4, DoD-9  
Location: [pst_materializer.rs](<C:/dev/Dedupe/crates/pst-dedup-cli/src/pst_materializer.rs:219>), [production.rs](<C:/dev/Dedupe/crates/pst-writer/src/production.rs:734>), [production.rs](<C:/dev/Dedupe/crates/pst-writer/src/production.rs:2878>)

`open_attachment_data`/payload probe failures are added as `AttachMetaFailed`, which sets the message-level `attach_list_failed` flag. The writer then emits a synthetic `ATTACH_META_FAILED` event at attach index `0`, while the same attachment remains in the DTO and may produce the actual `ATTACH_STREAM_OPEN_FAILED` or `ATTACH_STREAM_READ_FAILED` event.

This can inflate `attachments_failed`, duplicate ledger rows, and misidentify the attachment when a message has multiple attachments.

Correction: restrict `attach_list_failed` to actual `list_attachments` failure, or carry per-attachment probe failure state/reason through the attachment DTO and avoid the synthetic message-level event.

### DoD matrix

| DoD | Result |
|---|---|
| 1 Taxonomy | Partial — probe failure classification is incorrect |
| 2 Locus events | Partial — synthetic event may point to index 0 |
| 3 Ledger file | Met |
| 4 Invariant | Partial — mechanical event accounting passes, but attach outcome accounting can duplicate |
| 5 Histogram | Met |
| 6 Omit ≠ fail | Met |
| 7 Zero-byte success | Met |
| 8 Promote | Met via `D-0073-promote` |
| 9 Partial honesty | Partial — per-message counts can be inflated |
| 10 unique-eml | Met via `D-0073-eml` |
| 11 Exit honesty | Met |
| 12 CSV injection | Met |
| 13 Row cap | Met |
| 14 source ID | Met |
| 15 Documentation | Met |
| 16 Tests | Reported green by orchestrator; not independently runnable here |

### Verification evidence

Observed:

- `git diff --check` passed.
- `cargo fmt --all --check` passed.
- Targeted Cargo tests/clippy were blocked by read-only access to `target\debug\.cargo-build-lock`.

Reported by orchestrator:

- `writer_fidelity`: 33 passed.
- `unique_pst`: 17 passed.
- `unique_export_report`: 11 passed.
- Scoped clippy: passed.

Ledgerful status/impact was unavailable because its database/report paths could not be opened under the read-only environment. No files or Git state were modified.