# 0103 — Recipient TC SLBLOCK NID Order — Plan

> Phased checklist mapped to `spec.md` §7. Planning-only Phase 0 is **closed**. Do not implement until the user says Implement.
>
> **Ledger (implement):** `ledgerful ledger start crates/pst-writer --category BUGFIX --message "0103 recipient TC SLBLOCK NID order"`
>
> **Fold-in (2026-08-28):** `opencode-review.md` + `agy-review.md` → spec §2.9 / `foldin-note.md`. `seven_bit` mirror + exact SLBLOCK counts; Phase 2a preamble; Learn/PDF hedge; emit-sort two-counter doc-comment.

---

## Phase 0 — Spec expand → Ready (closed 2026-08-28)

- [x] Re-read MS-PST subnode BTree / SLBLOCK (PDF v20220215 §2.2.2.8.3.3; Learn HTML 404 this pass). Outlook search-by-NID is the working model; CI proves on-disk order.
- [x] Live: `insert(0)` still at `production.rs` ~4711; `add_subnode_leaf` does not sort; reader is linear.
- [x] Locked fix: **trailing push** of matrix NID + **emit-sort** in `add_subnode_leaf` (fail closed on duplicate NIDs). Decline allocate-matrix-first. Decline reader binary search.
- [x] Deferred §9; last-PR comments (#95–#92 empty; origin #90 is this track). Nothing minted. 0104 not stolen.
- [x] Status **Ready — not started**.
- [x] Fold-in: `seven_bit` arithmetic; exact `cEntries` 3/4; Phase 2a preamble; Learn v11.2 + hedge; two-counter doc-comment.

---

## Phase 1 — Writer emit → DoD-1

File: `crates/pst-writer/src/production.rs` (re-verify line numbers at execute; plan-time `main` @ `8e0e434`).

- [ ] In `build_recipient_table_strategy_a`, replace

  ```rust
  table_subs.insert(0, (matrix_nid, matrix_bid, 0));
  ```

  with

  ```rust
  table_subs.push((matrix_nid, matrix_bid, 0));
  ```

  Keep `matrix_nid = next_subnode_nid(&mut sub_counter)` **after** the row loop (cells first, matrix last). Do **not** hoist matrix NID allocation.

- [ ] In `Layout::add_subnode_leaf` (~5463):

  1. Copy `entries` to a local `Vec`.
  2. `sort_by_key(|(nid, _, _)| *nid)`.
  3. Adjacent equal NIDs → `Err(WriterError::Layout(...))` naming the duplicate (hex).
  4. Encode the **sorted** vec (`btype=0x02`, `cLevel=0x00`, `cEntries`, padding, SLENTRY 24-byte records).
  5. Keep the existing `payload.len() > MAX_BLOCK_DATA` fail-closed (~340 entries).

  Do not mutate the caller’s slice. Do not `unwrap`/`expect`.

- [ ] Doc-comment on `add_subnode_leaf`:
  - MS-PST subnode BTree is searched by NID (BTree-family inference; the SLBLOCK section does **not** say `rgentries MUST be sorted`). This function emits **strictly increasing** SLENTRY keys. CI proves on-disk order, not COM Outlook.
  - Duplicate NIDs are a layout error. **Today’s callers cannot hit that error:** recipient-table cells+matrix share one monotonic `sub_counter`; message leaves mix `subnode_counter` (type 0x1F), `attach_nid_counter` (type 0x05), and fixed `0x671`/`0x692` — disjoint low 5 bits. The guard is defense against a future bug, not a live green-test flip.

- [ ] Do **not** edit `pst-reader`, `pst-dedup-cli`, GUI, attach-table TC builder, `PagedHeapBuilder`, RowsPerBlock, or BCC filter.

---

## Phase 2 — Tests → DoD-2

### 2a. Unit (`production.rs` `#[cfg(test)] mod tests`)

There is **no** existing `add_subnode_leaf` unit test. Copy only the `Layout::new()` + find-block-by-BID-in-`layout.blocks` pattern from `write_data_chain_*` (`production.rs` ~5842). The four production call sites (~3380, ~3554, ~3840, ~4714) are not tests.

- [ ] `add_subnode_leaf_emits_nids_ascending` — call with `[(0x9F, 1, 0), (0x3F, 2, 0), (0x5F, 3, 0)]` (dummy bidData; they need not be real data BIDs for this encode test). Payload:

  - `[0] == 0x02`, `[1] == 0x00`
  - `cEntries` LE u16 at `[2..4] == 3`
  - SLENTRY nids at offsets `8`, `32`, `56` are `0x3F`, `0x5F`, `0x9F`
  - matching bidData `2`, `3`, `1` travel with those nids (prove sort moved whole entries, not just keys)

- [ ] `add_subnode_leaf_duplicate_nid_errors` — two entries with NID `0x3F` → `WriterError::Layout`. Message contains the NID (hex or decimal; pick one and assert `contains`).

### 2b. Fidelity (`crates/pst-writer/tests/writer_fidelity.rs`)

Reuse `recipient_table_subnode`, `list_subnode_entries`, `list_recipients`, `scratch_path` / `cleanup`. UTF-16 divert threshold stays **1025** chars (`MAX_HEAP_VALUE_SIZE` 2048).

- [ ] Extend `recipient_tc_long_string_cell_nid_round_trips` (~2858): after the existing `list_recipients` asserts, open the PST, `list_subnode_entries` on recip `bid_sub` (helper already returns it). **Do not change** the fixture strings (long display, short email/smtp). `seven_bit` copies display, so this leaf is already **2 cell NIDs + matrix**. Assert:

  - `!bid_sub.is_null()`
  - `subs.len() == 3`
  - `subs.windows(2).all(|w| w[0].nid.0 < w[1].nid.0)`
  - `table.info().hnid_rows as u64` is among `subs[].nid.0` (load via existing `load_from_table_bids` pattern in the 140-row test)

- [ ] New `recipient_tc_two_cell_nids_slblock_sorted`:

  ```rust
  let long = "N".repeat(1025);
  // display_name = long, email_address = long, smtp_address = None (or short).
  // Do NOT also set smtp to 1025 chars — that adds a fifth SLENTRY.
  ```

  Diverted cells: display + `seven_bit` (mirrors display) + email = 3, plus matrix = **4**. RecordKey 16 B / EntryId 24 B / SearchKey ASCII stay HID. `list_recipients`: one row; display and email equal the inputs. SLBLOCK: `len == 4`, strictly increasing NIDs.

- [ ] Do **not** weaken 140-row / empty / BCC / span-matrix tests. Optional (not required): HID-only 140-row SLBLOCK `len == 1`.

No `cargo test --test unique_pst` requirement. No INC*.

---

## Phase 3 — Docs → DoD-3

- [ ] `docs/unique-pst-export.md` **Recipient TC Strategy A (0100)** paragraph (~543): after the row-matrix / multi-page HN sentence, add: recipient-table `bid_sub` SLBLOCK entries are **NID-ascending** (0103). `insert(0)` of the matrix NID is forbidden; `add_subnode_leaf` sorts and fail-closes on duplicate NIDs. Residual list still names attach-table TC and HNBITMAPHDR — **drop** SLBLOCK order from residuals.
- [ ] `docs/pst-writer-fidelity-v1.md` recipient-table row (~33): one clause that cell-NID + matrix SLENTRYs are written in NID order (0103).
- [ ] `CHANGELOG.md` Unreleased: unique-pst recipient-table SLBLOCK NIDs are sorted so Outlook can resolve long-string cell NIDs (0103 / `D-0100-slblock-nid-order`).
- [ ] `docs/deferred.md`: mark `D-0100-slblock-nid-order` **closed / 0103** (on implement complete; this planning pass only notes the owner is Ready).

---

## Phase 4 — Finalize → DoD-4

- [ ] `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test -p pst-writer` (unit + `recipient_tc_*`); workspace tests before publish.
- [ ] Write `review.md` in this track dir: results, evidence, no new deferred (row closed).
- [ ] Update `../conductor.md`: this track **Completed**. Light `sequencing.md` / `ROADMAP.md`.
- [ ] Commit the implement ledger transaction (`BUGFIX` on `crates/pst-writer`).
- [ ] Notify: Series P 0099–0103 complete unless a new residual is found. No BCC track. Frontend stays **0105+**. Next free ID **0104** is unused — do not steal it for frontend.

---

## Handoff notes

- Planning-only until Implement. Product crates unchanged in this pass.
- Single-exe / no-daemon constraint unchanged (writer library only).
- Rollback: revert `insert(0)`→`push`, `add_subnode_leaf` sort, tests, docs. No on-disk summary schema change.
- Do not “fix” this by teaching `pst-reader` to binary-search while leaving the leaf unsorted.
- Do not hoist `matrix_nid` before the row loop.
- Do not change attach-table TC heap bytes.
- Do not chase `C:\dev\Dedupe-plan.md` (absent).
- `add_subnode_leaf` is `pub` on `Layout` in `production.rs`; unit tests belong in that file’s `mod tests`.
- Sorting message-level SLBLOCKs (0x671 / 0x692 / body / attach NIDs) is a **desired** side effect of the emit invariant. After sort, type-0x05 attach NIDs move **before** same-index type-0x1F body NIDs; `0x671`/`0x692` sort last. Tests must key by NID / `nid_type`, not vec index. Live `recipient_table_subnode` already `find`s by `nid_type`.
- Hotspot `crates/pst-dedup-cli/tests/export_exit_0078.rs` is **out of scope**.
