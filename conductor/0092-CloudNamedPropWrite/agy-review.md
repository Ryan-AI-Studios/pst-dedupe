# Antigravity Review — Track 0092: Cloud Named-Prop NPMAP Write

- **Track ID:** `0092-CloudNamedPropWrite`
- **Reviewer:** Antigravity (Advanced Agentic Pair Programmer)
- **Date:** 2026-08-24
- **Review Scope:** Review only (no implementation) — plan audit, MS-PST §2.4.7 specification compliance, Outlook compatibility, and writer risk analysis.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0092-CloudNamedPropWrite/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0092-CloudNamedPropWrite/plan.md)

---

## 1. Executive Summary

Track 0092 upgrades the production Unicode PST writer from an empty Named Property Map stub (`NID_NAME_TO_ID_MAP` = `0x61`) to an **allowlisted Name-to-ID map builder**. This enables cloud/modern pointer attachments (OneDrive / SharePoint web-references) to emit real `PSETID_Attachment` named properties (`PidNameAttachmentProviderType`, etc.) on attachment PCs, ensuring visibility in Outlook MAPI property inspectors while maintaining offline-only honesty (no hydration). This closes `D-0084-cloud-named-prop-write`.

Writing MS-PST internal tables is the highest-risk surface in the codebase. This review examines **MS-PST §2.4.7 Name-to-ID Map requirements**, **Outlook/ScanPST compatibility risks**, and **allowlist bounds**.

---

## 2. Blind Spots & Technical Findings

### Finding 0092-1: MS-PST §2.4.7 Stream vs Hash Bucket Requirements
- **Protocol Analysis:** MS-PST §2.4.7 specifies four distinct data structures on the Name-to-ID Map Property Context (`0x61`):
  1. `PidTagNameToIdGuidStream` (`0x0002`, `PtypBinary`): 16-byte GUID array for custom property sets.
  2. `PidTagNameToIdEntryStream` (`0x0003`, `PtypBinary`): 8-byte `NAMEID` descriptor records.
  3. `PidTagNameToIdStringStream` (`0x0004`, `PtypBinary`): Length-prefixed UTF-16LE string table (4-byte alignment).
  4. Hash Buckets (`0x1000` to `0x103E` / `0x1000` array): 63 bucket properties used by Outlook to perform O(1) named property lookups.
- **Blind Spot in Spec:** §2.2 and §2.4 mention GUID, Entry, and String streams, but do not specify whether Hash Buckets (`0x1000`–`0x103E`) must be populated.
- **Outlook Compatibility Hazard:** While `pst-reader` scans the Entry Stream directly, Outlook's native MAPI provider or Microsoft `scanpst.exe` may validate bucket consistency or fail named-prop resolution if hash buckets are omitted or inconsistent.
- **Recommendation:** 
  - For a minimal allowlisted set (e.g. 1–3 properties), implement standard MS-PST named-prop hashing (`ComputeNamedPropHash(guid, lid/name) % 63`) to populate the bucket array on PC `0x61`.
  - Validate the generated PST against `scanpst.exe -no repair` and `pst-reader::name_id_map()`.

### Finding 0092-2: Allowlist Scope & Property Set IDs
- **Locked Allowlist Definition:**
  - Property Set GUID: `PSETID_Attachment` `{96357F7F-59E1-47D0-99A7-46515C183B54}`
    (Byte representation in MS-PST: `[0x7F, 0x5F, 0x35, 0x96, 0xE1, 0x59, 0xD0, 0x47, 0x99, 0xA7, 0x46, 0x51, 0x5C, 0x18, 0x3B, 0x54]`)
  - Target Named Properties:
    1. `"AttachmentProviderType"` (`PtypString`, `0x001F`): e.g. `"OneDrivePro"`, `"OneDriveConsumer"`.
    2. `"AttachmentUrl"` / `"AttachmentProviderEndpointUrl"` (when present on source attach).
    3. `"AttachmentPermissionType"` (`PtypInteger32`, `0x0003`, when present).
- **NPID Assignment Invariant:**
  - Tag indices start at `NPID_BASE = 0x8000`.
  - Entry 0 (`prop_idx = 0`) -> Tag `0x8000`.
  - Entry 1 (`prop_idx = 1`) -> Tag `0x8001`.
- **Anti-Scope Creep Rule:** The writer MUST NOT attempt to clone arbitrary unknown named properties from source PSTs. It must strictly adhere to the allowlist.

### Finding 0092-3: Dual Support for Fixture Writer and Production Streaming Writer
- **Live Code Fact:**
  - `crates/pst-writer/src/lib.rs` (line 1248) builds fixture PSTs using `build_pc`.
  - `crates/pst-writer/src/production.rs` (line 1272) builds production unique PSTs using `build_pc_v2`.
- **Recommendation:** Build a reusable `NamedPropMapBuilder` module in `pst-writer` (leveraging the encoding helpers `encode_nameid_entry` and `encode_string_stream_entry` from `pst-reader`) and use it in both `lib.rs` and `production.rs`.

### Finding 0092-4: Fidelity Contract QC Update (`fidelity_contract_v1`)
- **Impact on 0080 QC:**
  - In `crates/pst-dedup-cli/src/unique_pst_qc.rs`, `PidNameAttachmentProviderType` is currently recorded under `known_gap` / `DroppedByDesign`.
  - Once Track 0092 ships, when a source message has `AttachmentProviderType` and the unique PST preserves it, the differential QC must verify its presence on the output attach PC.
  - DoD-4 must require updating `fidelity_contract_v1` to verify round-trip preservation.

---

## 3. Recommended Spec & Plan Amendments

1. **Update §2.4 Allowlist:** Lock the exact set of 3 named properties (`AttachmentProviderType`, `AttachmentUrl`, `AttachmentPermissionType`).
2. **Update Plan §Phase 1:** Specify creation of `NamedPropMapBuilder` with hash bucket calculation conforming to MS-PST §2.4.7.
3. **Update §7 Definition of Done (DoD-1 & DoD-4):**
   - Assert `pst-reader::NameIdMap` round-trips the output PST map.
   - Assert `scanpst.exe` (when available) reports zero defects on the Name-to-ID map.
   - Assert `fidelity_contract_v1` verifies `AttachmentProviderType` on output.

---

## 4. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready with protocol hash bucket & allowlist specifications)**
- **Complexity / Risk:** Medium-High (MS-PST on-disk binary structures and Outlook MAPI parser sensitivity).
- **Execution Estimate:** 2 – 3 days. Recommend running after 0088–0091.
