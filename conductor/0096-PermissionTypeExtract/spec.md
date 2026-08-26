# 0096 — Attachment PermissionType Extract

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0096-PermissionTypeExtract
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series N
- **Cross-repo contract:** n/a
- **Status:** Completed (Codex r4 PASS 2026-08-26)
- **Depends on:** 0084 · 0092 (all **Completed**)
- **Spec authored:** 2026-08-25
- **Series:** N (Operator fidelity — INC0102784 post-0092)
>
> **Review fold-in (2026-08-26):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.7. Extract is **four crates**; MS-OXCMSG value table is **0/1/2** (open-world preserve of other i32s). unique-eml unchanged.

---

## 1. Objective

Extract source `PidNameAttachmentPermissionType` (PSETID_Attachment named prop) into
canonical / `WriteAttachment.cloud_permission_type` so the **0092 writer path** that
already emits PermissionType can do so when present (MAY-if-present; never invent).

**Closes:** `D-0092-permission-type-extract`.

---

## 2. Context (read before starting)

### 2.1 Why now

| Fact | Detail |
|---|---|
| 0092 | Writer emits PermissionType when `cloud_permission_type: Some` (`PcValue::I32`); NPMAP allowlist ready |
| Adapter | `write_attachment_from_canonical{,_owned}` hardcodes `cloud_permission_type: None` (`production.rs` ~1217, ~1246) |
| Reader | 0084 resolves ProviderType only (`named_prop.rs` `NAME_ATTACHMENT_PROVIDER_TYPE` / `attachment_provider_type_npid`). Writer already has `NAME_ATTACHMENT_PERMISSION_TYPE`; **reader does not** |
| Bridge | `pst_materializer.rs` ~609–622 maps `AttachmentInfo` → `CanonicalAttachment` (`cloud_provider` / `cloud_url` already flow; permission does not) |
| INC0102784 | **0** attach-table `cloud_provider` rows — track is still correct (synthetic fixture); not blocked on this corpus |

### 2.2 Live code snapshot (verified 2026-08-26)

| Surface | State |
|---|---|
| `WriteAttachment.cloud_permission_type` | `Option<i32>` exists; written only in `write_cloud_link_pointer_attach` |
| `NamedPropWritePlan` | Inserts `AttachmentPermissionType` when `cloud_permission_type.is_some()` |
| `CanonicalAttachment` | Serialize; `cloud_provider` / `cloud_url` with `serde(default, skip_serializing_if)`; **no permission field** |
| `AttachmentInfo` | `cloud_provider` / `cloud_url` only |
| QC `MessageContentDetail.attaches` | Positional **6-tuple** `(filename, size, mime, hash, provider, method)` (`export_oracle.rs` ~556) |
| ProviderType QC | Payload-less cloud-pointer branch only (`unique_pst_qc.rs` ~1852–1882). Persisted `content_digests_v1` hardcodes provider empty (~1598) |
| Hasher | `attachment_meta_strings` is `filename:size` only (`hasher.rs` ~443). `is_cloud_link` may enter attach-content identity; provider/url/permission do **not** |
| `pc.get_i32` | Exists. **`get_i16` does not** |
| Fidelity contract | `PidNameAttachmentProviderType` Preserved; PermissionType is only mentioned in `cloud_modern_attachments` wording |

### 2.3 Product locks

1. **MAY-if-present only** — do not invent PermissionType. Extract when the named prop is on the attach PC; absence is not a defect.
2. **Open-world i32:** preserve the integer **as-is**. Do not reject out-of-range values. Documented MS-OXCMSG table is 0/1/2 (None/View/Edit); other values still round-trip.
3. ProviderType MUST path from 0092 unchanged.
4. No network hydrate; ledger URL honesty unchanged.
5. **Emit NPMAP only when allowlisted props are actually written** (0092). Plan-insert for PermissionType must stay aligned with the **cloud-pointer write path**, not a classic attach that happens to carry `Some` in the DTO then drops it.
6. **Write PermissionType on cloud-pointer rows only** (`is_cloud_link` → `write_cloud_link_pointer_attach`). Extract may be unconditional; drop at write for non-cloud is expected. Contract wording: *preserved on cloud pointer rows when source had it*.
7. **Identity isolation:** the new field must **not** enter `attachment_meta_strings` or `attach_content_hash` inputs (no keep-set regroup).
8. Keep-set JSON back-compat: `#[serde(default, skip_serializing_if = "Option::is_none")]` so older `keep_set_summary.json` still loads.
9. Fixtures in CI; INC* has no attach-table cloud rows — synthetic only. No production `unwrap`/`expect`.

### 2.4 Spec anchors (Phase 0 closed — pinned)

Microsoft Learn, accessed **2026-08-26**:

| Item | Value |
|---|---|
| Name string | `"AttachmentPermissionType"` |
| Property set | PSETID_Attachment `{96357F7F-59E1-47D0-99A7-46515C183B54}` (already `PSETID_ATTACHMENT` in `named_prop.rs`) |
| Type | **PtypInteger32** (`0x0003`) — [MS-OXCMSG §2.2.2.28](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxcmsg/38b5c935-6e3e-402e-95f7-2f765d3dabae) |
| Values | **0** None · **1** View · **2** Edit |
| OXPROPS | [MS-OXPROPS §2.371](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxprops/57fe0b09-beb4-44bb-9976-f2f856b3f1b1) |

Tests **must assert** name-string bytes, PSETID GUID bytes, and PtypInteger32 write (`PcValue::I32`). 0084-era NPMAP parse tests remain the external stream-format anchor (writer-bootstrapped round-trip alone cannot catch a shared wrong name).

Optional reader/writer constants `PERMISSION_NONE=0 / VIEW=1 / EDIT=2` for fixtures — **not** a reject list.

**Not canonical:** Organization/Anonymous 3–6. Those numbers are **not** in MS-OXCMSG §2.2.2.28. If a corpus has them, still preserve (lock 2).

### 2.5 Four-crate wiring (locked)

| Crate | Change |
|---|---|
| `pst-reader` | `NAME_ATTACHMENT_PERMISSION_TYPE` + `attachment_permission_type_npid()`; `AttachmentInfo.cloud_permission_type: Option<i32>`; `list_attachments_inner` resolve NPID **once per list** beside provider; `pc.get_i32(npid)` |
| `dedup-engine` | `CanonicalAttachment.cloud_permission_type` with serde default/skip |
| `pst-dedup-cli` | `pst_materializer.rs` map `att.cloud_permission_type`; QC compare + `fidelity_contract` row; live-read DoD-1 |
| `pst-writer` | Stop hardcoding `None` in `write_attachment_from_canonical{,_owned}` |

Miss the materializer mapping and DoD-1 silently fails (field populated nowhere).

**Out of this wiring:** unique-eml / GUI. Field may exist on canonical; those paths ignore it.

### 2.6 QC + fixture (locked)

**Contract:** add `PidNameAttachmentPermissionType` **Preserved** (mirror `PidNameAttachmentProviderType`). Absence on source is not a defect. Compare **cloud pointer rows when source had a value**.

**Compare surface:** `MessageContentDetail.attaches` 6-tuple **must grow**. Prefer a small `AttachDetail` struct (and `OutAttachSlot`) while extending — do not add a 7th positional field with mixed types.

**Known limits (document; do not silently over-claim):**

1. Persisted `content_digests_v1` has **no** provider/permission — permission QC is **live-read only**. DoD-1 must not rely on stale digest rows.
2. ProviderType compare today fires only for **payload-less** attaches. Permission compare follows the same branch unless Phase 2 cheaply generalizes. Payload-bearing attach with PermissionType is **not** written (lock 6) so live QC of that class is N/A.

**Fixture:** writer-generated source PST (`cloud_permission_type: Some(1)`, cloud pointer) → reader → canonical → writer → QC live-read. Plus constant-byte tests (§2.4) so a shared wrong NPMAP name cannot green-pass.

**Negative:** non-cloud / no-permission volume still emits **empty NPMAP stub** at `0x61` (no phantom `AttachmentPermissionType` entry).

### 2.7 Dual-AI review disposition (2026-08-26)

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | Pin MS-OXCMSG §2.2.2.28 / OXPROPS §2.371; PtypInteger32; 0/1/2; preserve any i32; do not validate as reject | opencode | **Agree** | §2.4; lock 2 |
| O2 | Four crates; materializer is the extract bridge; serde skip for keep-set JSON | opencode | **Agree** | §2.5; lock 8 |
| O3 | QC 6-tuple must grow; digest path has no provider; compare is payload-less; DoD-1 = live-read | opencode | **Agree** | §2.6 |
| O4 | Extract on PC presence; write only cloud-pointer rows; word the contract that way | opencode | **Agree** | locks 5–6 |
| O5 | Writer-bootstrapped fixture is self-consistency risk; assert spec constants + keep 0084 NPMAP tests | opencode | **Agree** | §2.6 fixture |
| O6 | Must not enter hasher / attach_content_hash | opencode | **Agree** | lock 7 |
| O7 | §8 must test `pst-dedup-cli` and `dedup-engine` | opencode | **Agree** | §8 |
| O8 | `PidNameAttachmentOriginalPermissionType` unmentioned | opencode | **Agree — out of scope** | §4 |
| A1 | `get_i32` then `get_i16` fallback | agy | **Decline as DoD.** Spec is PtypInteger32; `get_i16` does not exist. Residual if a real corpus stores i16 | — |
| A2 | End-to-end DTO list (named_prop → AttachmentInfo → CanonicalAttachment → materializer → from_canonical*) | agy | **Agree** | §2.5 |
| A3 | Emit-only-when-used; empty stub when unused | agy | **Agree** | lock 5; DoD-2 |
| A4 | QC + fidelity_contract when source had value | agy | **Agree** (live-read / cloud-pointer limits in §2.6) | §2.6 |
| A5 | INC* has 0 cloud rows; synthetic fixtures | agy | **Agree** | §2.1; lock 9 |
| A6 | Canonical values 3–6 Organization/Anonymous | agy | **Decline as spec table.** Learn §2.2.2.28 is 0/1/2 only. Open-world preserve if seen | §2.4 |

**Declined / not locked**

- i16 type fallback as a requirement.
- Documenting 3–6 as MS-OXCMSG canonical values.
- Permission QC via persisted content-digest rows.
- Writing PermissionType on non-cloud / payload-bearing attaches.
- unique-eml nested or GUI surfaces.
- `SoftSkipAttachRecord` permission column (MAY if cheap; not DoD).
- Coupling NPID helpers to **0097**.

---

## 3. In scope

1. Reader resolve of `AttachmentPermissionType` (`get_i32`) when present; `AttachmentInfo` field.
2. Canonical + materializer + `write_attachment_from_canonical*` population (`serde` default/skip).
3. Fidelity contract row + QC compare on **cloud pointer / live-read** (struct-ify attach tuples while extending).
4. Constant-byte tests for name / PSETID / PtypInteger32; writer-bootstrapped round-trip; negative empty-NPMAP stub.
5. Close `D-0092-permission-type-extract`.

## 4. Out of scope

- Full named-prop encyclopedia.
- `PidNameAttachmentOriginalPermissionType` (MS-OXPROPS §2.370 / MS-OXCMSG §2.2.2.27).
- Cloud hydrate (`D-0067-cloud*`).
- Body-cloud hosts / truncate honesty (`0097` / `D-0088-usgovcloud-microsoft-tld`).
- PtypInteger16 reader fallback (unless a later corpus proves it).
- unique-eml / GUI permission display.
- Hasher / identity preimage changes.

## 5. Preconditions & dependencies

- **P1:** 0092 Completed (writer + NPMAP emit path exist).
- *Verified:* extract is the missing half; INC* will not exercise it.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Prop absent on most corpora | Synthetic writer-bootstrapped fixture; operator optional |
| Shared wrong NPMAP name greens round-trip | Constant-byte tests + 0084 stream-format tests |
| QC looks green on digest path with empty provider | DoD-1 live-read only; document digest gap |
| Permission on classic attach dropped at write | Lock 6; contract scoped to cloud pointers |
| Hash regroup | Lock 7; regression that hashes are unchanged |
| 7-tuple positional bugs | `AttachDetail` struct |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 —** Synthetic cloud-pointer fixture with `AttachmentPermissionType = 1` (View) round-trips unique-pst **or** writer fidelity **and** QC **live-read** (not `content_digests_v1`). Output attach PC has PtypInteger32 named prop; `embedded` N/A. Name-string / PSETID GUID / I32 tests pass.
- [ ] **DoD-2 —** Absent PermissionType → no invent; non-cloud volume still empty NPMAP stub (no phantom PermissionType). Parent `content_hash` / `strong_content_hash` unchanged vs extract-off (field not in hasher).
- [ ] **DoD-3 —** `D-0092-permission-type-extract` **closed**. `PidNameAttachmentPermissionType` Preserved on cloud pointer rows when source had it.
- [ ] **DoD-4 —** Tests + clippy `-D warnings` + `fmt` for **all four** crates (§8).
- [ ] **DoD-5 — Recorded:** `review.md`; conductor **Completed**; ledger commit (`FEATURE`).

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy -p pst-reader -p dedup-engine -p pst-writer -p pst-dedup-cli --all-targets -- -D warnings
cargo test -p pst-reader
cargo test -p dedup-engine
cargo test -p pst-writer
cargo test -p pst-dedup-cli --test unique_pst_qc_0080
cargo test -p pst-dedup-cli --test unique_pst
```
