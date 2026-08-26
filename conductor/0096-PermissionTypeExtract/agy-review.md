# Antigravity Review — Track 0096: Attachment PermissionType Extract

- **Track ID:** `0096-PermissionTypeExtract`
- **Reviewer:** Antigravity (Advanced Agentic Pair Programmer)
- **Date:** 2026-08-26
- **Review Scope:** Review only (no implementation) — plan audit, blind spot discovery, MS-OXCMSG / MS-OXPROPS protocol analysis, and DTO extraction plumbing.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0096-PermissionTypeExtract/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0096-PermissionTypeExtract/plan.md)

---

## 1. Executive Summary

Track 0096 completes the missing extraction half of the `AttachmentPermissionType` cloud attachment workflow. In Track 0092, `pst-writer` implemented allowlisted Named Property Map (NPMAP) writing for `PSETID_Attachment` properties, including `AttachmentProviderType` (string) and `AttachmentPermissionType` (integer `i32`). However, `from_canonical_message` and `from_canonical_message_owned` currently hardcode `cloud_permission_type: None` because `pst-reader` and `CanonicalAttachment` did not yet extract or carry the permission type property.

This track connects the reader to the writer, enabling round-trip preservation of modern cloud attachment permissions (e.g. View / Edit / Organization permissions) and closing `D-0092-permission-type-extract`.

---

## 2. Technical Context & Protocol Anchors

### 2.1 MS-OXCMSG / MS-OXPROPS `PidNameAttachmentPermissionType`
- **Property Set:** `PSETID_Attachment` (`{96357F7F-59E1-47D0-99A7-46515C183B54}`)
- **Property Name:** `"AttachmentPermissionType"`
- **Property Type:** `PtypInteger32` (`0x0003`)
- **Canonical Values:**
  - `0` = None
  - `1` = View (Read-Only)
  - `2` = Edit (Read-Write)
  - `3` = Organization View
  - `4` = Organization Edit
  - `5` = Anonymous View
  - `6` = Anonymous Edit
- When Outlook creates a modern/cloud web-reference attachment (e.g. OneDrive or SharePoint share), it populates both `AttachmentProviderType` (e.g. `"OneDrivePro"`) and `AttachmentPermissionType` on the attachment Property Context (PC).

---

## 3. Blind Spots & Technical Findings

### Finding 0096-1: Robust Integer Type Fallback in `pst-reader`
- **Reader Constraint:**
  - While standard Outlook clients store `AttachmentPermissionType` as `PtypInteger32` (`0x0003`), third-party exporters or legacy MAPI implementations may store the flag as `PtypInteger16` (`0x0002`).
  - If `pst-reader` only calls `pc.get_i32(perm_npid)`, any `PtypInteger16` record would evaluate to `None`.
- **Recommendation:**
  - In `pst-reader/src/messaging/attachment.rs`, attempt:
    ```rust
    let perm_type = pc.get_i32(perm_npid).ok().flatten()
        .or_else(|| pc.get_i16(perm_npid).ok().flatten().map(|v| v as i32));
    ```
  - This ensures robust extraction across diverse MAPI implementations.

### Finding 0096-2: End-to-End DTO Wiring Flow
- **Plumbing Trace:**
  1. `pst-reader/src/messaging/named_prop.rs`: Add `NAME_ATTACHMENT_PERMISSION_TYPE: &str = "AttachmentPermissionType"` and `NameIdMap::attachment_permission_type_npid(&self) -> Option<u16>`.
  2. `pst-reader/src/messaging/attachment.rs`: Add `pub cloud_permission_type: Option<i32>` to `AttachmentInfo`.
  3. `dedup-engine/src/keepset.rs`: Add `pub cloud_permission_type: Option<i32>` to `CanonicalAttachment` (`#[serde(default, skip_serializing_if = "Option::is_none")]`).
  4. `pst-dedup-cli/src/pst_materializer.rs`: Map `att.cloud_permission_type` into `CanonicalAttachment.cloud_permission_type`.
  5. `pst-writer/src/production.rs`: Update `from_canonical_message` and `from_canonical_message_owned` to map `a.cloud_permission_type` into `WriteAttachment.cloud_permission_type` (stopping the hardcoded `None`).

### Finding 0096-3: Emit-Only-When-Used Invariant Enforcement
- **Integrity Rule:**
  - Track 0092 established the strict rule: "Emit allowlisted named properties on `NID_NAME_TO_ID_MAP` (`0x61`) only when actually used by attachments in the volume; emit empty stub when unused".
  - If a source archive has zero cloud attachments (or cloud attachments without permission types), `cloud_permission_type` must remain `None`, and the writer must NOT emit an unreferenced `AttachmentPermissionType` entry into the destination NPMAP.
  - Test vectors must verify that non-cloud PSTs continue to produce an empty NPMAP stub on `0x61`.

### Finding 0096-4: Source-Differential QC Reconciliation
- **QC Verification:**
  - In `crates/pst-dedup-cli/src/unique_pst_qc.rs`, source-differential compare and clean-room QC inspect attachment properties.
  - When a source message contains `cloud_permission_type: Some(perm)`, QC must verify that the output PST contains `cloud_permission_type: Some(perm)`.
  - Update `fidelity_contract_v1` to record `AttachmentPermissionType` as verified when present.

### Finding 0096-5: Operator Corpus Reality (INC0102784)
- **Context:**
  - INC0102784 contained 0 attach-table `cloud_provider` or `cloud_permission_type` rows (all 374 attach failures were method-5 embedded messages addressed in Track 0094).
  - Validation for Track 0096 should rely primarily on synthetic round-trip unit and integration fixtures (`writer_fidelity.rs` and `unique_pst_qc_0080.rs`).

---

## 4. Recommended Spec & Plan Amendments

1. **Update `plan.md` §Phase 1 (Reader & Canonical):**
   - Add `NAME_ATTACHMENT_PERMISSION_TYPE` and `attachment_permission_type_npid` to `pst-reader::named_prop`.
   - Add `cloud_permission_type: Option<i32>` with `i32`/`i16` fallback to `AttachmentInfo` and `CanonicalAttachment`.
   - Update `pst_materializer` and `from_canonical_message*` to pass `cloud_permission_type`.
2. **Update `plan.md` §Phase 2 (QC & Contract):**
   - Update `fidelity_contract_v1` and `unique_pst_qc` to verify round-trip `PermissionType` preservation.
   - Close `D-0092-permission-type-extract`.
3. **Update §7 Definition of Done (DoD-1 & DoD-2):**
   - Assert positive round-trip: synthetic PST with `AttachmentPermissionType` preserves value in output PST.
   - Assert negative round-trip: synthetic PST without `AttachmentPermissionType` produces clean empty NPMAP stub without phantom property emission.

---

## 5. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready with integer type fallback & end-to-end DTO plumbing)**
- **Complexity / Risk:** Low (straightforward DTO field wiring; writer path already implemented in 0092).
- **Execution Estimate:** 0.5 – 1 day.
