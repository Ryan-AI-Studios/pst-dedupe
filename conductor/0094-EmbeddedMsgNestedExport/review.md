# 0094 EmbeddedMsgNestedExport — Review

**Status:** Completed (engineering)  
**Branch:** `track/0094-EmbeddedMsgNestedExport`  
**Ledger TX:** `4e881bbe-001b-4e17-a529-ffb7c595c4ca` (FEATURE)

## Summary

Method-5 nested unique-pst export is wired end-to-end:

- `NestedCanonicalMessage` + `#[serde(skip)]` on `CanonicalAttachment`
- Winner-only `materialize_nested_for_winner` with depth owner = writer `max_embedded_depth`
- `PcValue::Object` writes `PidTagAttachDataObject` PtypObject `0x3701`/`0x000D`
- Reader property-first resolve + 0069-era scan fallback
- Child by-value streams via `open_attach_data_from_message_node` keyed by `(source_path, nid)`
- Depth/budget → `ATTACH_DEPTH_LIMIT` via `embedded_extract_limit` / `embedded_depth_limited`
- QC method-5 presence match; `AttachDigestEntry.attach_method` for clean-room digests
- Closes **D-0069-embed-object**; narrows **D-0067-embedded-depth** (unique-eml MIME / matter children residual)

## Codex reviews

| Round | Verdict | Notes |
|---|---|---|
| r1 | FAIL | P1 multi-source NID key; P2s (flags, 0x3701 assert, stream test, HTML budget, method-5 probe, partial honesty) |
| r2 | FAIL | Aggregate BODY+HTML preflight; corrupt recipient honesty |
| r3 | FAIL | HTML Err honesty; attachments_incomplete → MetaFailed; digest v1 compatibility |
| r4 | FAIL | Persist attach_method on clean-room digests |
| r5 | **PASS WITH DEFERRED P3** | Operator INC* re-smoke / finalize residuals only |

Raw: `review.codex.md`, `review.codex.r2.md` … `review.codex.r5.md`.

## DoD

| Item | Result |
|---|---|
| DoD-1 PtypObject + resolve + nested fields + parent hash | Met (fidelity + keepset hash regression) |
| DoD-2 unparsed + nest child stream | Met |
| DoD-3 depth → ATTACH_DEPTH_LIMIT | Met |
| DoD-4 deferred closes | Met |
| DoD-5 INC* operator note | **Residual** — expect large drop in `ATTACH_EMBEDDED_UNPARSED`, not necessarily zero; optional Outlook nest open |
| DoD-6 review / conductor / ledger | Met at ship |

## Gates (orchestrator)

- `cargo fmt --all` / clippy `-D warnings` on pst-reader, dedup-engine, pst-writer, pst-dedup-cli
- `cargo test -p pst-writer --test writer_fidelity -- embedded` → 6/6
- `cargo test -p pst-dedup-cli --test unique_pst` → 31/31
- `cargo test -p pst-dedup-cli --test unique_pst_qc_0080` → 58/58
- Parent-hash + message_nodes key unit tests OK

## Operator re-smoke (DoD-5 residual)

Re-run unique-pst on Desktop `INC0102784*.pst` with the new binary; compare `attachments_failed_by_reason` vs prior 374× `ATTACH_EMBEDDED_UNPARSED`. Expect a **large drop**, not a guaranteed zero. Optional Outlook open of a nested message is evidence only.

Recorded as deferred operator item **D-0094-inc-resmoke**.
