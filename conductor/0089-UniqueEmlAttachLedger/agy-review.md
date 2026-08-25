# Antigravity Review — Track 0089: Unique-EML Attach Ledger Parity

- **Track ID:** `0089-UniqueEmlAttachLedger`
- **Reviewer:** Antigravity (Advanced Agentic Pair Programmer)
- **Date:** 2026-08-24
- **Review Scope:** Review only (no implementation) — plan audit, blind spot discovery, architecture/locus analysis, and improvement recommendations.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0089-UniqueEmlAttachLedger/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0089-UniqueEmlAttachLedger/plan.md)

---

## 1. Executive Summary

Track 0089 bridges the fidelity gap between `unique-pst` and `unique-eml` by adding full CSV attachment failure ledgering (`export_attachments.csv`) to `unique-eml`. This closes `D-0073-eml`.

Currently, `unique-eml` tracks a basic integer counter (`manifest.stats.attach_parts_failed`) to drive exit code 64 / fidelity classification, but lacks the locus-level audit trail (`source_id`, `msg_nid`, `attach_nid`, `reason_code`, `cloud_url`, etc.) required by eDiscovery operators.

This review identifies critical **architectural boundaries** between `dedup-engine` and `pst-dedup-cli`, **Mode A soft-skip capture gaps**, and **report path placement rules**.

---

## 2. Blind Spots & Technical Findings

### Finding 0089-1: Architectural Decoupling of Event Collection vs Ledger Sink
- **Blind Spot in Plan:** `plan.md` §Phase 1 states: "Construct `AttachLedgerSink` in `run_unique_eml` ... Enqueue from the same events/counters that feed `attach_parts_failed`."
- **Live Code Constraint:** `write_canonical_eml` is executed in `dedup-engine/src/eml_pack.rs`. `dedup-engine` is a core engine crate and **cannot depend on `pst-dedup-cli`** (where `AttachLedgerSink` resides).
- **Hazard:** Attempting to pass `AttachLedgerSink` directly into `eml_pack.rs` would violate crate boundaries or force unwanted crate refactoring.
- **Recommended Solution:** 
  - Enhance `EmlWriteResult` in `dedup-engine/src/eml_pack.rs` to return a list of structured attachment event descriptors:
    ```rust
    pub struct EmlAttachEvent {
        pub attach_index: u32,
        pub filename: String,
        pub size: u64,
        pub attach_method: Option<i32>,
        pub reason_code: String,
        pub error_detail: Option<String>,
    }
    ```
  - In `pst-dedup-cli/src/unique_eml_cmd.rs`, iterate over `wres.attachment_events` and enqueue them into `AttachLedgerSink`. This maintains clean crate boundaries with zero circular dependencies.

### Finding 0089-2: Mode A Soft-Skip Ingestion Must Not Be Overlooked
- **Blind Spot in Spec:** Mode A (`--promote-on-attach-fail`) promotes a clean peer over a winner with failed attachments. The promoted peer itself succeeds, but the soft-skipped attachments on the loser message must still be documented in the ledger.
- **Live Code Fact:** In `unique_pst_cmd.rs` (lines 2127–2160), `resolved.soft_skip_attach_records` are drained and fed into `attach_ledger`.
- **Recommendation:** Explicitly mandate in `plan.md` (Phase 1) and DoD-2 that `run_unique_eml` must process `resolved.soft_skip_attach_records` and call `ledger.mark_promoted_winner()`, mirroring `unique-pst` line-for-line.

### Finding 0089-3: Report Artifact Layout & Directory Resolution
- **Blind Spot in Spec:** In `unique-pst`, reports land in `--out/REPORTS/` (or `--report-dir`). In `unique-eml`, `--out` is the directory containing `VOL001/`, `manifest.json`, and `summary.json`.
- **Ambiguity:** Where does `export_attachments.csv` land for `unique-eml`?
- **Recommendation:** Lock the path in Phase 0:
  - If no separate `--report-dir` is specified, `export_attachments.csv` lands in `--out/export_attachments.csv` (or `--out/REPORTS/export_attachments.csv` if `unique-eml` aligns with the reports subfolder convention).
  - Explicitly document this in `docs/unique-pst-export.md` and CLI help.

### Finding 0089-4: Ledger Path Mode & Basename Options
- **Finding:** Track 0081 introduced `--ledger-path-mode` (`full` vs `basename`) and `source_id` integer indices.
- **Recommendation:** Ensure `UniqueEmlCliArgs` exposes `--ledger-path-mode` so operators using unique EML packs in multi-custodian workflows have identical path masking and source join capabilities.

---

## 3. Recommended Spec & Plan Amendments

1. **Plan §Phase 1 Update:** Specify `EmlWriteResult.attachment_events` as the clean DTO boundary between `eml_pack` and `AttachLedgerSink`.
2. **Plan §Phase 1 Update:** Include `soft_skip_attach_records` wiring and `mark_promoted_winner` calls.
3. **DoD Update:** Add explicit assertion that `export_attachments.csv` contains identical header columns to `unique-pst` (`source_id`, `source_path`, `folder_path`, `msg_nid`, `attach_nid`, `attach_index`, `filename`, `size`, `attach_method`, `reason_code`, `severity`, `error_detail`, `cloud_provider`, `cloud_url`, `message_subject`).

---

## 4. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready with architectural clarification on DTO boundary)**
- **Complexity / Risk:** Low-Medium (straightforward wiring once DTO boundary is respected).
- **Execution Estimate:** 1 – 1.5 days.
