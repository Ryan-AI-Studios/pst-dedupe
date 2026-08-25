# 0092 — Cloud Named-Prop NPMAP Write — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-24):** production.rs is the live stub; MUST ProviderType; hash buckets; emit-only-when-used; scanpst DoD — see `spec.md` §2.6.

> **Ledger:** `ledgerful ledger start crates/pst-writer --category FEATURE --message "0092 cloud named-prop NPMAP write"`

---

## Phase 0 — Design lock → DoD-2 (partial)

- [ ] Re-read 0084 named-prop resolve (`NAME_ATTACHMENT_PROVIDER_TYPE` + `PSETID_ATTACHMENT`).
- [ ] Cite MS-OXCMSG §2.2.2.9 + MS-PST §2.4.7 (access date).
- [ ] Lock MUST vs MAY-if-present allowlist (`spec.md` §2.4).
- [ ] Lock NPID sorted-allowlist assignment.
- [ ] Lock emit-when-used (not always-present map).
- [ ] Inventory **both** stubs: `production.rs` (ship) and `lib.rs` (fixtures).

## Phase 1 — NamedPropMapBuilder → DoD-1, DoD-3

- [ ] Implement builder: GUID/entry/string streams + hash buckets `0x1000`–`0x103E`.
- [ ] Reuse `pst-reader` encode helpers where possible.
- [ ] Wire production + fixture writers.
- [ ] Reader round-trip unit tests (synthetic).
- [ ] No production `unwrap`/`expect`.

## Phase 2 — Attach PC props → DoD-2

- [ ] Write ProviderType on cloud pointer attaches when known.
- [ ] MAY copy Url / PermissionType only if present on source.
- [ ] Keep classic LongPathname; optional Pathname 0x3708.
- [ ] Ledger URL mandatory path unchanged.

## Phase 3 — QC + docs → DoD-4, DoD-5

- [ ] Update `fidelity_contract_v1` (ProviderType preserved when source had it).
- [ ] scanpst-on-copy when available (skip-safe).
- [ ] Close/narrow `D-0084-cloud-named-prop-write`.
- [ ] Operator note: Outlook visibility still offline-pointer only; optional `qc_attestation_v1`.

## Phase 4 — Finalize → DoD-6

- [ ] `review.md`; conductor **Completed**; ledger commit.

---

## Handoff notes

- Longest pole / highest writer risk — after 0088–0091 unless counsel blocks on Outlook visibility.
- Hard allowlist; refuse encyclopedia.
- Never mutate source PSTs; never hydrate.
- Do not treat reader-only goldens as Outlook proof.
