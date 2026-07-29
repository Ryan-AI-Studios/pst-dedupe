# 0080 Implementation Notes — Unique PST Output QC

Branch: `track/0080-unique-pst-outlook-qc`  
Pending ledger tx: `2ca8160c-1909-4f79-98ab-af2ac3e54c6f` (`crates/pst-dedup-cli`)

## What shipped

### Phase 0 — `fidelity_contract_v1`
- Module: `crates/pst-dedup-cli/src/fidelity_contract.rs`
- Allowlist classifier: absent property ⇒ `unexplained_loss`
- Statuses: `preserved` | `best_effort` | `dropped_by_design`
- Q7: `PidTagDisplayCc` = preserved; `PidTagDisplayBcc` / `display_bcc` = dropped_by_design
- Q10: `cloud_modern_attachments` + `PidNameAttachmentProviderType` explicit blind-spot lines

### Phase 7 — CC write path
- `WriteMessage.display_cc`; `from_canonical_message` / `_owned` map CC; count non-empty BCC as dropped
- Writer emits `PidTagDisplayCc` (0x0E03)
- Unit test: `display_cc_written_and_bcc_counted_dropped`

### Phases 1–4 — Tier A QC
- Modules: `unique_pst_qc.rs`, reuse `export_oracle::{structural_digest_pst, message_content_detail, hex_sha256}`
- Levels: `off|structure|sample|full` (default **sample**)
- Risk-weighted deterministic sampling (`select_sample_indices`, default max 64)
- Artifacts: `qc_report_v1.json`, `qc_findings.csv`, `content_digests.json`
- Hard findings ⇒ `verify_ok = false` ⇒ existing `VERIFY_FAILED` (no new exits)
- `qc_ms` on `PhaseTimings`
- `parents_only` / `--no-attachments`: missing attaches are explained, not defect
- Degraded winners: content digests may be explained (CRC/body flags)

### Standalone `qc-pst`
- CLI name **`qc-pst`** (not `qc` — matter-produce)
- Output-only when sources gone; `content_digest_backed` when digests present

### Phases 5–6 — sidecars
- `qc_external.rs`: BYOB independent reader (counts only); scanpst discovery + local temp + `-no repair` + log parse + timeout/kill + `.bak` hard error
- Stub `.cmd` tests — CI needs no Outlook/libpff

### Phase 9
- `qc_attestation.rs` — load/record only; never self-attests
- `docs/unique-pst-export.md` client-retirement section (dated 2026-07-28)
- `docs/deferred.md`: D-0068-02 / D-0071-operator-outlook / D-0074-e2e-fixture closed; D-0080-* residuals present

## Tests
- `crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs` — negative truncate, sample determinism, fixture sample/full zero hard findings, qc-off skip
- Unit: fidelity contract, sampling, scanpst stubs, attestation

## Residuals
- D-0080-scanpst-arg (real Outlook build token confirm)
- D-0080-external-reader-matrix
- D-0080-cloud-attachments (named-prop resolution)
- D-0080-bcc-policy / recipient-table / newoutlook / com-declined

## Operator smoke (DoD-19)
- scanpst: **absent in CI** — skip-safe with reason; stub tests cover bak/timeout/ok paths
- Real Outlook scanpst evidence remains operator-local (D-0080-scanpst-arg)

## Not done by implementer
- Final `review.md` (orchestrator after Codex)
- Mark track Completed (leave In Progress for review)
