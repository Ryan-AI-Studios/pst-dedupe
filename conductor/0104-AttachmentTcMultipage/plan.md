# 0104 — Attachment TC Multipage — Plan

> Phased checklist; map each phase to `spec.md` §7. Phase 0 is **closed** in the spec.
> Execute in `C:\dev\Dedupe`. Do not re-open recipient Strategy A, BCC, or HNBITMAPHDR.

> **Ledger (implement):** `ledgerful ledger start crates/pst-writer --category FEATURE --message "0104 attach-table TC Strategy A multipage"`

---

## Phase 0 — Preconditions → DoD-3/4 (closed in spec)

- [x] Live `build_attachment_table_tc` is single-page + `bid_sub = 0` (`production.rs` ~3767 / ~4340 @ `4becdfe`).
- [x] 0100 helpers exist: `PagedHeapBuilder`, `alloc_tc_value`, `write_row_matrix_tree`, `load_from_table_bids`.
- [x] 0103 `add_subnode_leaf` sorts; do not `insert(0)` the matrix NID.
- [x] MS-PST: table optional; row count MUST match attach objects.
- [x] Plan-time width **25** / RowsPerBlock **327** (re-verify at execute).
- [x] Last-PR Cursor comments disposed (§2.8 — none to mint).
- [x] Status **Ready — not started**.

Re-verify at execute: `row_width` via a 1-row `table` / `build_template_tc_columns`; `HeapBuilder` still used at the call site; `list_attachments` still 0x05-enum.

---

## Phase 1 — Writer Strategy A → DoD-1

- [ ] Add `AttachmentTableBuilt { heap, table_bid_sub, extra_content_bytes }` (or equivalent).
- [ ] `build_attachment_table_strategy_a(layout, rows)`:
  - `PagedHeapBuilder::new(0xBC)`
  - own `sub_counter` + `table_subs` (do **not** share attach object NID counter)
  - `alloc_tc_value` for each filename
  - RowIndex BTH + TCINFO as today
  - `write_row_matrix_tree`; `table_subs.push` matrix; `add_subnode_leaf`
  - patch `hnidRows` to matrix NID
- [ ] `build_message_pc`: only call when `written_attaches` is non-empty; push `(NID_ATTACHMENT_TABLE, table_bid, table_bid_sub)`; add extra bytes into `written_content_bytes`.
- [ ] **Delete** `build_attachment_table_tc` **and** `heap_data_len` (only consumer of the helper is that builder’s unused second return). `table_len` = `built.heap.len()` like the recipient path. Do not leave either as dead code (`-D warnings`).
- [ ] Generalize `PagedHeapBuilder` module docs + HNBITMAPHDR error from “recipient TC” to “TC heap”. Do not implement bitmap pages.
- [ ] Extend `add_subnode_leaf` doc-comment: attachment-table cells+matrix share one monotonic `sub_counter` (0104), same class as recipient.
- [ ] Fail closed if A cannot store all rows. No truncate event.

---

## Phase 2 — Tests → DoD-2

- [ ] Helper `attachment_table_subnode(path, msg_nid) -> (Vec<u8>, BlockId)` copying `recipient_table_subnode` (`NidType::AttachmentTable`).
- [ ] Switch `per_message_attachment_table_rows_and_row_index` to `load_from_table_bids`.
- [ ] `attachment_tc_many_rows_round_trips` — **200** attaches, names **≥20 BMP chars** (`attach_filename_test_{i:04}.txt`). Heap **> 8176**. Do not use `file_000.txt`.
- [ ] `attachment_tc_matrix_spans_rows_per_block` — **328** short names (`RowsPerBlock+1` if width drifted).
- [ ] `attachment_tc_long_filename_cell_nid` — 1025-char filename; SLBLOCK strictly increasing; `len >= 2`.
- [ ] Confirm template + zero-attach tests still pass without Strategy A on those paths.
- [ ] Keep `message_size_uses_real_attachment_table_size` green (strictly larger).

Tiny by-value payloads (`b"x"`). Distinct filenames so `get_row_string` is checkable. 328-row test may keep short names (matrix span, not HN paging).

---

## Phase 3 — Docs → DoD-3

- [ ] `docs/unique-pst-export.md`: Strategy A attach table; drop attach-table from the 0100 residual sentence; keep HNBITMAPHDR residual. One sentence: six columns = template MUST; extra message-table properties optional / not added.
- [ ] `docs/pst-writer-fidelity-v1.md` attachment-table row: matrix subnode + paged HN + cell NID for >2048-byte names.
- [ ] `CHANGELOG.md` one-liner under Unreleased.
- [ ] `docs/deferred.md`: `D-0093-attachment-tc-page` **closed / 0104**. Leave `D-0100-hn-bitmap-hdr` open (note attach heaps share the fail-closed).

---

## Phase 4 — Finalize → DoD-4

- [ ] Workspace gate + `ledgerful verify` (see spec §8). `CARGO_TARGET_DIR=C:\dev\Dedupe\target` if fixture discovery fails.
- [ ] Write `review.md` in this track dir: results, evidence, and any explicitly-deferred items.
- [ ] Update `../conductor.md`: set this track's status to **Completed**.
- [ ] Commit the ledger transaction in the execution repo (`FEATURE` / `crates/pst-writer`).
- [ ] Notify: Series P 0099–0104 complete unless a new residual is found. Next free ID **0105** (frontend / Hermes only if started — do not steal).

---

## Handoff notes

- `/implement-track` publishes (PR → `main`, squash). Force-add `conductor/0104-*/` (`conductor/` is gitignored).
- Do not silently cap attach-table rows.
- Do not steal 0105+ for this work; do not use 0104 for frontend (this track **is** 0104).
- Do not commit INC* or `output/`.
- Single-exe / no-daemon unchanged.
- Trailing **push** of matrix NID; 0103 sort is the emit invariant.
- Nested messages go through `build_message_pc` too — they inherit Strategy A; no separate nested path.
