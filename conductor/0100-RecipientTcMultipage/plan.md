# 0100 — Recipient TC Multipage — Plan

> Phased checklist; map each phase to `spec.md` §7. Phase 0 is **closed** in the spec.
> Execute in `C:\dev\Dedupe`. Do not re-open Strategy B as production.

> **Ledger:** `ledgerful ledger start crates/pst-writer --category FEATURE --message "0100 recipient TC Strategy A multipage"`

---

## Phase 0 — Preconditions → DoD-3/4 (closed in spec)

- [x] MS-PST row-matrix-as-subnode + RowsPerBlock + HID 11-bit per page (spec §2.3).
- [x] Fail-closed when A cannot store all included rows (no cap).
- [x] Reader `load_tc` concat bug called out as in-scope.
- [x] Last-PR Cursor comments disposed (0102 mint; D-0097 P3).
- [x] Status **Ready — not started**.

Re-verify at execute: [MS-PST] §2.3.4.4 formulas, live `row_width` / RowsPerBlock (**56 / 146**; plan-time 145 was off-by-one), `HeapBuilder` HID encoding, four SLENTRY-concat sites still present.

---

## Phase 1 — Shared `TableContext::load` → DoD-4

Do this **before** or in the same PR as the writer so intermediate unique-pst files remain readable.

- [ ] Move row-matrix + cell-HNID resolution into `TableContext::load` (or a helper it always uses). Pass `bid_sub` + a subnode resolver (`find_subnode_entry` + `read_block_data`); do **not** accept a pre-concatenated `Vec` of all SLENTRYs.
- [ ] Update all four call sites: `load_tc`, `list_recipients`, embedded attach table, embedded recipient table. None may pre-concat.
- [ ] RowsPerBlock leaf walk; ignore dead space; exact row count.
- [ ] `get_row_string`: HID → heap; NID → table subnode bytes (`InvalidHid` path today).
- [ ] Tests: extra sibling subnode does not change row count; HID cells still work.

## Phase 2 — Writer Strategy A → DoD-1, DoD-2, DoD-3, DoD-5

- [ ] Multi-page HN for recipient-table node data only. HID `((block as u32) << 16) | ((hid_index as u32) << 5)`, `hid_index` 1-based per page, HNPAGEHDR. Fail closed on HNBITMAPHDR page indices.
- [ ] `build_recipient_table_tc`: no `keep` loop; all included rows; `hnidRows` NID when non-empty; table `bid_sub` set.
- [ ] **Empty path:** `hnidRows = 0`, skip `try_alloc` of empty matrix, `bid_sub = 0`.
- [ ] **New:** per-row string &gt; `MAX_HEAP_VALUE_SIZE` → cell NID (not present in this builder on `main`).
- [ ] Row-matrix data tree with integral rows per 8176-payload block (not naive `write_data_chain` on a flat vec).
- [ ] Delete / invert `build_recipient_table_budget_aware` production use. Keep To→Cc→Bcc **ordering**.
- [ ] Replace `recipient_tc_budget_truncates_*` with 140-row full-write assertions; **add >RowsPerBlock (e.g. 160) matrix test**; empty-table `hnidRows==0`; optional >2048-char cell NID test.
- [ ] Do not change `build_attachment_table_tc` **writer** behavior.
- [ ] **Keep** `recipient_tc_truncate_event_is_known_gap_not_defect`.

## Phase 3 — QC + docs → DoD-1, DoD-6

- [ ] CLI QC: 140-row write → no `recipient_table` defect; BCC known_gap unchanged; injected truncate event still known_gap (branch retained).
- [ ] `docs/unique-pst-export.md`: Strategy A; residual attach-table + bitmap-page.
- [ ] `docs/pst-writer-fidelity-v1.md`: same.
- [ ] `docs/deferred.md`: close `D-0093-recipient-tc-multipage`; append `D-0100-hn-bitmap-hdr` if unimplemented.

## Phase 4 — Finalize → DoD-7

- [ ] Workspace gate + `ledgerful verify` (see spec §8). `CARGO_TARGET_DIR=C:\dev\Dedupe\target` if fixture discovery fails.
- [ ] `review.md`; conductor **Completed**; ledger commit.
- [ ] Optional HITL: INC* unique-pst to `output/inc0102784-post-0100/` (not git). Outlook open of the **synthetic** 140-row PST is enough if INC* is skipped; record which.

---

## Handoff notes

- `/implement-track` publishes (PR → `main`, squash). Force-add `conductor/0100-*/`.
- Do not silently restore the 48-row cap.
- Do not steal 0101 depth, 0102 oracle attest, or attach-table overflow.
- Do not commit INC* or `output/`.
- Single-exe / no-daemon unchanged.
