# 0080 Implementation Notes — Unique PST Output QC

Branch: `track/0080-unique-pst-outlook-qc`  
Ledger tx (review fixes): `6a02f056-cc09-4ed9-bb5e-164326f601d5`

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
- Output-only when sources gone; `content_digest_backed` only when loaded digests are **source-origin**

### Phases 5–6 — sidecars
- `qc_external.rs`: BYOB independent reader (counts only); scanpst discovery + local temp + `-no repair` + log parse + timeout/kill + `.bak` hard error
- Stub `.cmd` tests — CI needs no Outlook/libpff
- Independent reader Ok counts compared to expected volume message count (and folder leaf floor)

### Phase 9
- `qc_attestation.rs` — load/record only; never self-attests
- `docs/unique-pst-export.md` client-retirement section (dated 2026-07-28)
- `docs/deferred.md`: D-0068-02 / D-0071-operator-outlook / D-0074-e2e-fixture closed; D-0080-* residuals present

## Review fixes (validated internal review)

### 1. `folder_tree_matches` (DoD-4 Q3)
- Every expected leaf must match (suffix or equality, case-insensitive); **missing leaf ⇒ false**
- Residual Unique Mail allowance only when the *expected* path is itself residual Unique Mail
- No wholesale collapse acceptance (multi-leaf expected vs single Unique Mail fails)
- Tests: unit collapse reject; integration `folder_tree_collapsed_multi_leaf_hard_fails`

### 2. `content_digests` honesty (DoD-21)
- Persist `content_digests.json` **only** for source-side digests (`source_differential` + live source reads)
- File carries `"origin": "source"`; `origin=output` never enables `content_digest_backed`
- Output-only qc-pst without prior source digests: structural only; does **not** write output digests as `content_digests.json`
- Test: two output-only runs never set `content_digest_backed`

### 3. Independent reader counts (DoD-12)
- When reader returns Ok + message_count, mismatch vs volume `messages_written` ⇒ defect
- Folder count: defect if reader reports 0 / fewer than expected export leaf folders
- Stub test: wrong message count hard-fails

### 4. BCC `known_gap` on production path (DoD-15)
- `PreparedWinner.display_bcc` plumbed from `CanonicalMessage` before adapter drop
- Adapter `dropped` not discarded (sentinel if needed); `candidate_from_write_msg` carries BCC
- QC counts `known_gap` for non-empty BCC; never hard-fails alone
- Test: candidate with BCC ⇒ `known_gap > 0`, `hard_fail == false`

### 5. DoD-9 pipeline negatives
- `probe_unexplained_property` hook exercises unexplained_loss → hard_fail + CSV artifacts
- CC strip source-differential defect test
- Attach payload mismatch defect test
- `flip_byte` asserts hard_fail/defect (header flips)

### 6. Default-on safety
- Full QC green path **with attachments** (`fixture_unique_pst_qc_full_with_attachments_zero_hard_findings`)
- Uses `--allow-partial-fidelity` for fixture soft attach fails; asserts QC hard findings zero

### 7. Sampling cap
- When truncating to `sample_max`, **stratum representatives first** (volume-last / extremes survive)
- Test: `sample_cap_prefers_stratum_over_index_truncate`

### Also
- `qc-pst` honors `parents_only` from `summary.json` (`family_policy` / `attachments_omitted_by_policy`)

## Tests
- `crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs` — collapse tree, digest honesty, BCC known_gap, reader counts, attach/CC negatives, full+attach green, sample cap
- Unit: folder_tree, content origin, sampling strata, fidelity contract, scanpst stubs

## Residuals
- D-0080-scanpst-arg (real Outlook build token confirm)
- D-0080-external-reader-matrix
- D-0080-cloud-attachments (named-prop resolution)
- D-0080-bcc-policy / recipient-table / newoutlook / com-declined
- Fixture `aspose_outlook.pst` still has soft attach/CRC noise (partial fidelity) — QC explains ledger soft-fails; product export_risk remains separate

## Operator smoke (DoD-19)
- scanpst: **absent in CI** — skip-safe with reason; stub tests cover bak/timeout/ok paths
- Real Outlook scanpst evidence remains operator-local (D-0080-scanpst-arg)

## Not done by implementer
- Final `review.md` (orchestrator after Codex)
- Mark track Completed (leave In Progress for review)
