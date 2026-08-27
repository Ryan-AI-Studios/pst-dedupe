# 0098 — Template NID / Folder Contents Collision — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.

> **Ledger:** `ledgerful ledger start pst-writer --category BUGFIX --message "Skip reserved MS-PST template nidIndex values so folder contents TCs are not overwritten"`
> tx `fcdc105c-6185-4ea2-8af0-6968e7ec5930`

---

## Phase 0 — Diagnose → DoD-1 context

- [x] NBT vs `folders()`: 4055 vs 4005; all 50 orphans parent `0x602` (Purges).
- [x] Contents NID `0x60E` == `NID_CONTENTS_TABLE_TEMPLATE`; hierarchy `0x60D` / assoc `0x60F` same nidIndex.
- [x] `write_nbt` stable-sorts; reader HashMap last-wins empty template.
- [x] CI fixtures never reached nidIndex 0x30.

## Phase 1 — Writer skip + fail-closed → DoD-1, DoD-3

- [x] `RESERVED_NID_INDICES` + `alloc_nid` skip loop.
- [x] `used_nids` on `Layout`; `add_node` / `add_node_data` refuse duplicates.
- [x] `write_nbt` windows duplicate-key error.

## Phase 2 — Tests → DoD-2

- [x] `alloc_nid_skips_reserved_template_indices`
- [x] `add_node_data_rejects_duplicate_nid`
- [x] `preserve_paths_many_folders_does_not_clobber_contents_template_nid` (40 folders)

## Phase 3 — Docs + finalize → DoD-4

- [x] `docs/pst-writer-fidelity-v1.md` + `docs/deferred.md`
- [x] `review.md`; `conductor.md` Completed; ledger commit
- [x] Pin `DECISION:` in ai-brains
