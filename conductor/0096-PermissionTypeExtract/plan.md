# 0096 — Attachment PermissionType Extract — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-26):** four-crate wiring; pin MS-OXCMSG §2.2.2.28 (PtypInteger32, 0/1/2,
> open-world i32); extract always / write cloud-pointer only; QC live-read + `AttachDetail` struct;
> hasher isolation; no i16 fallback; OriginalPermissionType out — see `spec.md` §2.7.

> **Ledger:** `ledgerful ledger start crates/pst-reader --category FEATURE --message "0096 PermissionType extract"`
> (commit summaries must name all four crates: reader, engine, writer, cli).

---

## Phase 0 — Design lock (citations already pinned)

- [x] Treat `spec.md` §2.4 as closed: name `"AttachmentPermissionType"`, `PSETID_ATTACHMENT`, PtypInteger32, values 0/1/2.
- [x] Reuse `WriteAttachment.cloud_permission_type: Option<i32>` — do not invent a parallel field.
- [x] Confirm write gate = `is_cloud_link` only; NPMAP plan-insert stays aligned with that write.
- [x] Confirm hasher / `attach_content_hash` stay free of the new field.

## Phase 1 — Four-crate extract → DoD-1, DoD-2

- [x] `pst-reader` `named_prop.rs`: `NAME_ATTACHMENT_PERMISSION_TYPE` + `attachment_permission_type_npid()`.
- [x] `list_attachments_inner`: resolve permission NPID **once per list** beside provider; `pc.get_i32` only (no i16).
- [x] `AttachmentInfo.cloud_permission_type: Option<i32>`.
- [x] `CanonicalAttachment.cloud_permission_type` with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- [x] `pst_materializer.rs`: map `att.cloud_permission_type` (this is the extract bridge — do not skip).
- [x] `write_attachment_from_canonical` / `_owned`: stop hardcoding `None`.
- [x] Unit tests: exact name bytes, PSETID GUID bytes, `PcValue::I32`; 0084 NPMAP parse tests still green.
- [x] Optional `PERMISSION_NONE/VIEW/EDIT` constants for fixtures — not a reject list.

## Phase 2 — QC + contract → DoD-1, DoD-3

- [x] `fidelity_contract_v1`: `PidNameAttachmentPermissionType` **Preserved** (mirror ProviderType; absence on source is not a defect).
- [x] Grow `MessageContentDetail.attaches` via an **`AttachDetail` struct`** (and `OutAttachSlot`); do not add a 7th positional tuple field.
- [x] Compare permission on the **payload-less cloud-pointer / live-read** path. Document digest-row gap.
- [x] DoD-1: writer-bootstrapped cloud-pointer PST with `Some(1)` → unique-pst/QC live-read round-trip.
- [x] DoD-2: no-permission / non-cloud volume → empty NPMAP stub; parent hashes unchanged.
- [x] Close `D-0092-permission-type-extract`.

## Phase 3 — Finalize → DoD-4, DoD-5

- [ ] `review.md`; conductor **Completed**; ledger commit.
- [ ] Operator note: INC* will not exercise this (0 attach-table cloud rows).

---

## Handoff notes

- Last in Series N by design (no INC* attach-table cloud). Completes the 0092 MAY path.
- unique-eml / GUI ignore the field this track.
- Do not invent i16 reads or Organization/Anonymous as spec values.
- Do not put permission in identity hashes.
