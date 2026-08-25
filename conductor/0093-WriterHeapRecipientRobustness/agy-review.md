# Antigravity Review — Track 0093: Writer Heap + Recipient Robustness

- **Track ID:** `0093-WriterHeapRecipientRobustness`
- **Reviewer:** Antigravity (Advanced Agentic Pair Programmer)
- **Date:** 2026-08-25
- **Review Scope:** Review only (no implementation) — plan audit, blind spot discovery, MS-PST §2.3 Heap-on-Node analysis, and recipient TC robustness.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0093-WriterHeapRecipientRobustness/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0093-WriterHeapRecipientRobustness/plan.md)

---

## 1. Executive Summary

Track 0093 resolves a critical production writer failure discovered during operator unique-PST exports on real-world multi-mailbox archives (`INC0102784.pst`):
1. **Message PC Heap Page Overflow:** Messages with large `DisplayTo`, `DisplayCc`, `DisplayBcc`, `Subject`, `InternetMessageId`, or `SenderEmailAddress` strings exceeded the single-block 8176-byte Heap-on-Node capacity (`heap page overflow: 8192 > 8176`), aborting the write.
2. **Recipient Table Context (TC) Overflow:** Single-page recipient TCs overflow when an email contains dozens or hundreds of recipients (e.g. 136 recipients on INC0102784).

This track formalizes subnode string diversion (`PcValue::SubnodeString`) for non-body properties (closing/narrowing `D-0068-01`) and defines an honest, machine-readable recipient table capacity policy.

---

## 2. Technical Context & MS-PST Protocol Anchors

### 2.1 Heap-on-Node (HN) Limits (MS-PST §2.3.1)
- In Unicode PST files, a Heap-on-Node (HN) page lives inside a single data block with a maximum data capacity of **`MAX_BLOCK_DATA` = 8176 bytes** (`0x1FF0`).
- Inside the HN page, bytes are consumed by:
  - `HNHDR` header (12 bytes)
  - `HNPAGEMAP` allocation table: grows downward from the end of the block at **2 bytes per allocation** plus 4-byte overhead
  - B-Tree on Heap (BTH) root and leaf nodes for property records
  - Inlined property values
- When `MAX_HEAP_VALUE_SIZE` was set to 3580 bytes, a message containing two ~3 KB string properties (e.g. a long subject and a multi-recipient `DisplayTo`) pushed the total heap data + pagemap beyond 8176 bytes, triggering `WriterError::Layout("heap page overflow...")`.

### 2.2 Subnode String Support in `pst-reader`
- In MS-PST, variable-length properties (`PtypString` `0x001F`, `PtypBinary` `0x0102`) can store a subnode NID in `dwValueHnid` instead of an in-heap HID.
- `pst-reader` (`crates/pst-reader/src/ltp/pc.rs::resolve_value`) already checks `hid.hid_type() != 0` and resolves subnode strings transparently via `self.subnodes.get(&value_hnid)`.
- Therefore, diverting oversized non-body strings to subnodes in `pst-writer` (`PcValue::SubnodeString`) is **100% backward-compatible with `pst-reader` and native Outlook**.

---

## 3. Blind Spots & Technical Findings

### Finding 0093-1: Multi-Property Cumulative Heap Overflow Hazard
- **Blind Spot in Uncommitted Code / Spec:**
  - The local change lowers `MAX_HEAP_VALUE_SIZE` from 3580 to 2048 and diverts individual strings when `bytes.len() > MAX_HEAP_VALUE_SIZE`.
  - **Hazard:** Testing each property independently against a static 2048-byte limit does **not** protect against multiple medium-sized properties. For example, an email with:
    - Subject: 1,800 bytes (inlined)
    - Sender: 1,800 bytes (inlined)
    - DisplayTo: 1,800 bytes (inlined)
    - DisplayCc: 1,800 bytes (inlined)
    Total inlined data = 7,200 bytes + BTH overhead + HNPAGEMAP = **>8,176 bytes -> OVERFLOW**.
- **Recommendation:** Implement a **dynamic / cumulative heap budget** in `build_message_payload`. If the current heap payload plus the new string exceeds `MAX_HEAP_VALUE_SIZE` (or ~1,500 bytes cumulative), divert the string to a subnode regardless of its individual size.

### Finding 0093-2: Recipient Truncation Selection Order (To vs Cc vs Bcc)
- **Blind Spot in Recipient Cap:**
  - In `build_recipient_table_tc`, capping rows naively via `&rows[..MAX_INLINE_RECIPIENT_ROWS]` (48 rows) takes whichever recipients appeared first in the source array.
  - If a source PST lists `Cc` or `Bcc` entries before primary `To` recipients, essential `To` recipients risk being truncated while secondary recipients are retained.
- **Recommendation:**
  - When truncation is necessary, sort/filter candidate recipient rows so that `MAPI_TO` (recipient type 1) recipients are populated first, followed by `MAPI_CC` (type 2), and finally `MAPI_BCC` (type 3).
  - Emphasize in operator docs that `PidTagDisplayTo`, `PidTagDisplayCc`, and `PidTagDisplayBcc` on the parent Message PC are **never truncated** and retain the full recipient list in Outlook's header view.

### Finding 0093-3: QC Classification Mismatch (`Defect` vs `KnownGap`)
- **Live Code Conflict in `unique_pst_qc.rs`:**
  - The source-differential QC (`crates/pst-dedup-cli/src/unique_pst_qc.rs::compare_message_properties`, line 1669) checks `src_keys != out_keys` for `recipient_table`.
  - If a source email has 136 recipients and the output has 48, QC records a `QcFinding` with `class: FindingClass::Defect`, triggering `VERIFY_FAILED` (exit code 64).
- **Recommendation:**
  - Update `fidelity_contract_v1` in `unique_pst_qc.rs` so that when `out_recipients.len() == MAX_INLINE_RECIPIENT_ROWS` and `out_recipients` is a strict subset of `src_recipients`, the finding is classified as **`FindingClass::KnownGap`** with detail `"recipient table truncated to 48 rows due to single-page TC limit; DisplayTo/Cc intact"`.
  - This prevents operator runs from failing verification while remaining 100% honest about the limitation.

### Finding 0093-4: Strategy A (Multi-Page TC) vs Strategy B (Cap + Honesty)
- **Evaluation:**
  - **Strategy A (Multi-Page TC):** Requires implementing multi-block TC row matrices and 2-level BTH trees in `pst-writer`. This is a high-risk multi-day undertaking.
  - **Strategy B (Cap + Honesty):** Capping at 48 rows + structured telemetry (`recipient_tc_truncated_messages`) + `KnownGap` QC classification meets all immediate operator needs for INC0102784.
- **Recommendation:** Ship **Strategy B** in Track 0093 to unblock the release immediately, and spawn residual **`D-0093-multi-page-recipient-tc`** for future multi-page TC architecture.

### Finding 0093-5: Structured Telemetry & Ledger Reporting
- **Current State:** The uncommitted code only emits a runtime log `tracing::warn!("recipient TC truncated...")`.
- **Recommendation:**
  - Add `recipient_tc_truncated_messages: u64` and `recipient_rows_truncated: u64` to `ExportSummary` and `summary.json`.
  - Add an optional ledger reason `RECIPIENT_TC_TRUNCATED` on the message entry so downstream eDiscovery review platforms can audit affected documents.

---

## 4. Recommended Spec & Plan Amendments

1. **Update `spec.md` §2.3 & §3:**
   - Formalize Strategy B (48-row cap with `KnownGap` QC classification + telemetry) as the locked ship target.
   - Open `D-0093-multi-page-recipient-tc` for future unbounded multi-page TC row trees.
2. **Update `plan.md` §Phase 1:**
   - Add dynamic cumulative heap budgeting to `push_string_prop` to guard against multiple ~1.8 KB properties in a single message PC.
3. **Update `plan.md` §Phase 2:**
   - Add recipient priority sorting (`To` > `Cc` > `Bcc`) prior to row capping.
   - Update `crates/pst-dedup-cli/src/unique_pst_qc.rs` to classify recipient cap truncation as `KnownGap`.
   - Add `recipient_tc_truncated_messages` counter to `summary.json`.
4. **Update §7 Definition of Done (DoD-2):**
   - Assert that differential QC passes with `known_gap` (not `defect`) when tested against a 136-recipient synthetic fixture.

---

## 5. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready with essential cumulative heap & QC classification amendments)**
- **Complexity / Risk:** Low-Medium (straightforward writer string diversion and QC reconciliation).
- **Execution Estimate:** 1 – 1.5 days.
