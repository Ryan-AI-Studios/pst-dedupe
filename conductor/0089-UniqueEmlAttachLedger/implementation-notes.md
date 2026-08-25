# 0089 — Implementation notes

## Reason mapping (Phase 0 lock)

| Soft-fail cause | `reason_code` |
|---|---|
| `is_cloud_link` (any open/availability fail) | `ATTACH_CLOUD_LINK` |
| stream not available / missing `attach_nid` / null source / open fail / Io | `ATTACH_STREAM_OPEN_FAILED` |
| embedded open fail | `ATTACH_STREAM_OPEN_FAILED` (+ `embedded_message_unparsed`) |
| unmapped `EmlWriteError::Other` / `PathBudget` | `ATTACH_UNKNOWN` (never drop row) |

Do **not** use pack-manifest `ATTACH_PART_FAILED` / `REASON_ATTACH_PART_FAILED` as CSV `reason_code`.

## Architecture

- `EmlAttachEvent` + `attachment_events` on `EmlWriteResult` in `crates/dedup-engine/src/eml_pack.rs`.
- Populated only in `prepare_attachments` Err arm (sole `attachments_failed` increment site).
- CLI owns `AttachLedgerSink`; maps events → `AttachLedgerRow` via `enqueue_soft_skip_row`.
- CSV path: `{--out}/export_attachments.csv` (pack root). Header = `EXPORT_ATTACHMENTS_CSV_HEADER`.

## Mode A + fail-closed

- After finalize / keep_set: init sink; `mark_promoted_winner` for `promoted_from_failure`; drain `resolved.soft_skip_attach_records` with `winner_promoted=true`.
- Write loop: enqueue `wres.attachment_events` with volume enrichment + promoted flag from `promoted_winner_loci`.
- `finish()` before classify; ledger init/flush errors set `report_ok=false` (same honesty as unique-pst).

## Tests added

- Engine: soft-fail events + cloud-link mapping (`soft_attach_fail_*`, `soft_fail_cloud_link_maps_attach_cloud_link`).
- CLI lib: `soft_fail_eml_event_writes_export_attachments_csv_header`, `mode_a_soft_skip_and_promoted_winner_rows`, `attach_ledger_row_cap_truncated_marker`, `attach_ledger_init_fail_full_fail_closed`.
- Integration: `unique_eml_attach_ledger_csv_header_at_pack_root`, `unique_eml_attach_ledger_off_no_csv`.
- Exit: `unique_eml_ledger_off_still_exit_64_from_counters`.

## Residuals

- **D-0073-gui** still open (wizard attach-ledger UI).
- Orchestrator: Codex review → `review.md` → conductor Completed → ledger TX commit.
