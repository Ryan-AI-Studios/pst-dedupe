# 0092 — Cloud Named-Prop NPMAP Write

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0092-CloudNamedPropWrite
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M continuation
- **Cross-repo contract:** n/a
- **Status:** Completed (Codex luna r6 PASS WITH DEFERRED P3; 2026-08-25)
- **Depends on:** 0068 · 0069 · 0084 (all **Completed**); reader `named_prop` encode helpers available
- **Spec authored:** 2026-08-24
- **Series:** M (Unique export fidelity residuals — continuation)
>
> **Review fold-in (2026-08-24):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.7.

---

## 1. Objective

Replace the production writer’s **named property map stub** (NID `0x61`) with an **allowlisted** NPMAP write + attach PC named props so cloud/modern pointer attachments retain `PSETID_Attachment` / `PidNameAttachmentProviderType` visibility beyond classic tags (`PidTagAttachLongPathname` / method / filename), while keeping mandatory ledger URL honesty and never hydrating payloads.

**Closes:** `D-0084-cloud-named-prop-write` (may residual non-allowlisted named props / full encyclopedia).

---

## 2. Context (read before starting)

### 2.1 Why this track exists now

| Deferred / ceiling | Severity | Claim |
|---|---|---|
| **D-0084-cloud-named-prop-write** | P3 | Writer store NPMAP remains a stub; 0084 pointer preserve uses classic tags + ledger URL |
| Counsel risk | — | If classic tags are insufficient for URL visibility in Outlook, named-prop re-emit is the next lever |
| 0084 product | — | Detect ≠ hydrate; anti-ghost; Mode A incomplete for cloud — **unchanged** |

### 2.2 Live code snapshot (verified 2026-08-24) — **corrected**

| Surface | State |
|---|---|
| **Production** stub | `crates/pst-writer/src/production.rs` ~1271–1277 — empty PC via `build_pc_v2` on `NID_NAME_TO_ID_MAP` |
| **Fixture** stub | `crates/pst-writer/src/lib.rs` ~1247–1253 — empty PC via `build_pc` |
| Reader encode | `pst-reader` `encode_nameid_entry` / `encode_string_stream_entry` exported |
| Reader parse | `NameIdMap::from_streams` consumes GUID/entry/string; **ignores MS-PST hash table** (`named_prop.rs`) |
| 0084 unique-pst | Classic tags + `ATTACH_CLOUD_LINK` ledger columns |

### 2.3 Product locks

1. **Allowlist only** — no source NPMAP clone / encyclopedia.
2. **No network hydrate**; no invented binary attach data.
3. **Ledger URL remains mandatory** for cloud detects (0084).
4. **Emit NPMAP only when an allowlisted named prop is actually written.** Do not write a populated map on cloud-free exports (would churn every golden digest).
5. NPID assignment: `0x8000 + index` in **sorted allowlist order** (deterministic; 0087 ethos).
6. Synthetic fixtures in CI. **scanpst-on-copy** is a **DoD QC step** when `scanpst` is available (0080 skip-safe). Optional operator Outlook open via `qc_attestation_v1` — **not** CI-blocking (COM declined).
7. Update `fidelity_contract_v1` for props actually written; fail closed on unknown.

### 2.4 Allowlist (Phase 0 cites MS-OXCMSG §2.2.2.9)

**MUST (minimal set — MS-OXCMSG `afByWebReference` 0x7):**

| Named prop | Set | Notes |
|---|---|---|
| `PidNameAttachmentProviderType` (`"AttachmentProviderType"`) | `PSETID_Attachment` `{96357F7F-59E1-47D0-99A7-46515C183B54}` | Same entry 0084 already resolves. Values e.g. `OneDrivePro` / `OneDriveConsumer` when known |

**MAY if present on the source attach (do not invent):**

| Named prop | Notes |
|---|---|
| `AttachmentUrl` / `AttachmentProviderEndpointUrl` | Only copy if source had it; URL also remains on classic LongPathname + ledger |
| `AttachmentPermissionType` | Only copy if source had it |

**Classic tags (no NPMAP cost):** keep `PidTagAttachLongPathname`. **Optional cheap:** also write `PidTagAttachPathname` (0x3708) for older-client tolerance.

**Anti-scope:** do not add a third named prop “because Outlook might want it” without a citation + source presence.

### 2.5 MS-PST NPMAP layout (normative)

PC `0x61` must implement the streams Outlook/scanpst care about:

1. `PidTagNameToIdGuidStream` (0x0002)
2. `PidTagNameToIdEntryStream` (0x0003)
3. `PidTagNameToIdStringStream` (0x0004)
4. **Hash buckets** starting at `0x1000` with **`PidTagNameidBucketCount` = 251** (MS-PST §2.4.7.5 SHOULD; access 2026-08-25) — bucket index `((dwPropertyID ^ wGuidN) % 251)`; only non-empty buckets emitted

`pst-reader` ignoring the hash table means **DoD-1 reader round-trip is necessary but not sufficient.** Promote scanpst-on-copy (when present) to DoD.

Prefer a reusable `NamedPropMapBuilder` in `pst-writer` used by **both** production.rs and fixture `lib.rs`.

### 2.6 Dual-AI review disposition (2026-08-24)

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | Production stub is `production.rs`, not `lib.rs` | opencode | **Agree** | §2.2 |
| O2 | Minimal allowlist is `PidNameAttachmentProviderType` only (MS-OXCMSG) | opencode | **Agree as MUST** | §2.4; extra named props only if source had them |
| O3 | Optional classic `PidTagAttachPathname` | opencode | **Agree (optional cheap)** | §2.4 |
| O4 | Reader round-trip misses hash table; scanpst + optional Outlook attestation | opencode | **Agree** | lock 6; DoD-1/4 |
| O5 | Golden from real PST NPMAP bytes | opencode | **Partial** | Operator-local optional; CI synthetic — evidence policy |
| O6 | Deterministic NPID assignment order | opencode | **Agree** | lock 5 |
| O7 | Emit map only when allowlisted props used | opencode | **Agree** | lock 4 |
| A1 | Implement hash buckets §2.4.7 | agy | **Agree** | §2.5 |
| A2 | Lock 3 named props including Url + PermissionType always | agy | **Partial** | ProviderType MUST; others MAY-if-present — do not invent |
| A3 | Shared `NamedPropMapBuilder` for fixture + production | agy | **Agree** | §2.5 |
| A4 | `fidelity_contract_v1` must verify ProviderType when source had it | agy | **Agree** | DoD-4 |

---

## 3. In scope

1. Real NPMAP write (streams + hash buckets) when allowlisted props are used.
2. MUST re-emit `PidNameAttachmentProviderType` on cloud pointer attaches; MAY copy additional allowlisted named props if present on source.
3. Shared builder for production + fixture writers.
4. QC: reader NameIdMap round-trip; scanpst-on-copy when available; fidelity_contract update.
5. Close/narrow `D-0084-cloud-named-prop-write`.

## 4. Out of scope

- Full named-prop encyclopedia / arbitrary source NPMAP clone.
- Body-inline sovereign hosts (`0088`).
- Hydration / Graph download.
- Matter schema changes.
- Requiring a real client PST in CI.

## 5. Preconditions & dependencies

- **P1:** 0084 Completed (detect + classic preserve + ledger).
- *Verified:* production stub in `production.rs`; fixture stub in `lib.rs`; reader encode helpers exist.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Outlook/scanpst reject populated map without hash buckets | Implement §2.4.7 buckets; scanpst DoD when tool present |
| Writer+reader shared misunderstanding | scanpst + optional operator Outlook attestation; don’t rely on reader-only goldens |
| Scope creep | Hard allowlist; MAY-if-present only |
| Golden churn | Emit map only when used |

## 7. Definition of Done

- [ ] **DoD-1 — NPMAP:** When allowlisted named props are written, unique-PST `0x61` is a real map (GUID/entry/string **and hash buckets**) that `pst-reader` NameIdMap parses. Empty stub remains only when no allowlisted props are used.
- [ ] **DoD-2 — Attach PC:** Cloud pointer attaches re-emit `PidNameAttachmentProviderType` when known; classic LongPathname retained; no hydrate; ledger URL still required.
- [ ] **DoD-3 — Determinism:** NPID `0x8000+` in sorted allowlist order; cloud-free exports do not grow a populated map.
- [ ] **DoD-4 — QC:** `fidelity_contract_v1` verifies ProviderType when source had it (no longer `DroppedByDesign` in that case). `scanpst -no repair` on a local copy is run when the tool is available (skip-safe like 0080). Optional Outlook `qc_attestation_v1` noted in `review.md`.
- [ ] **DoD-5 — Deferred:** Close or narrow `D-0084-cloud-named-prop-write`.
- [ ] **DoD-6 — Recorded:** `review.md`; conductor **Completed**; ledger TX committed.

## 8. Verification commands

```powershell
cargo test -p pst-writer
cargo test -p pst-reader -- named_prop
cargo test -p pst-dedup-cli -- unique_pst
cargo fmt --all --check
cargo clippy -p pst-writer -p pst-reader --all-targets -- -D warnings
ledgerful verify
```
