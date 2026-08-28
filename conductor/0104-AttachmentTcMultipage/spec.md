# 0104 — Attachment TC Multipage (Strategy A)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open recipient Strategy A, BCC
> default, HNBITMAPHDR, or column-schema layout during implementation.

- **Track ID:** 0104-AttachmentTcMultipage
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `docs/unique-pst-export.md` + this track. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-28); do **not** chase it at execute.
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0100 (Completed — `PagedHeapBuilder`, `write_row_matrix_tree`, `alloc_tc_value`, shared `TableContext::load_from_table_bids`). 0103 (Completed — `add_subnode_leaf` NID-ascending emit-sort). 0069 (Completed — per-message `0x671` schema).
- **Spec authored:** 2026-08-28
- **Series:** P (Unique-PST defensibility)
>
> **Closes:** `D-0093-attachment-tc-page`.
> **HITL:** none required. Optional operator Outlook open of a synthetic unique-pst with 200+ attaches is evidence, not CI (`D-0094-inc-resmoke` stays operator).
>
> **Last-PR fold-in (2026-08-28):** PRs **#97, #96, #95, #94**. Disposition in §2.8. No Cursor/Bugbot comments in that window. **#90** Bugbot is **0103** (closed). This ID was unused; it is **not** stolen for frontend.
>
> **Review fold-in (2026-08-28):** `opencode-review.md` + `agy-review.md`. Disposition in §2.9 and `foldin-note.md`. Delete `heap_data_len` with the old builder; pin DoD-2b names to ≥20 BMP chars; Attachment Table Template leaf exists via toc.json.

---

## 1. Objective

Write **every successfully written attachment** as a row in the per-message attachment table (`NID 0x671`) so Outlook’s table and the attach-object subnodes stay 1:1, including messages whose filename HIDs + row matrix no longer fit a single 8176-byte heap page.

Today `build_attachment_table_tc` uses single-page `HeapBuilder`, puts the row matrix on that same heap, and sets message-level `bid_sub = 0`. ~100+ attaches (or long filenames) re-trip `heap page overflow` and fail the write. `PstFile::list_attachments` enumerates type-`0x05` PCs, so existing 1-row tests stay green while Outlook, which walks `NID_ATTACHMENT_TABLE`, can see a missing or truncated table.

This advances unique-export **defensibility**: a unique PST that contains attach objects whose table cannot name them (or that aborts the volume on a large attach list) is not affidavit-clean. Same-class fix as **0100** recipient Strategy A; attach-table was explicitly parked there.

---

## 2. Context (read before starting)

### 2.1 Diagnosis (`D-0093-attachment-tc-page`, still live)

Spawned from **0093** so recipient Strategy B would not silently leave attach-table overflow as a known-unknown. **0100** closed recipient multipage and locked “do not change `build_attachment_table_tc`.” **0103** sorted SLBLOCK emit and left this residual unchanged.

Deferred text (gist): `build_attachment_table_tc` allocates one filename HID + row per attach with **no byte budget**. ~100+ attaches or long filenames re-trip `heap page overflow`.

### 2.2 Live code snapshot (verified 2026-08-28, `main` @ `4becdfe`)

Re-verify line numbers at execute.

| Surface | State |
|---|---|
| Call site | `production.rs` `build_message_pc` (~3752–3767): builds `table_rows` from `written_attaches`, `HeapBuilder::new(0xBC)`, `build_attachment_table_tc`, `write_data_chain`, **`subnode_entries.push((NID_ATTACHMENT_TABLE, table_bid, 0))`**. |
| Builder | `build_attachment_table_tc` (~4340): filename `utf16le` HID via `heap.try_alloc`; row matrix HID on the **same** heap; RowIndex BTH; `hnidRows` patched as that HID. Empty `rows` still allocate a zero-length matrix HID. |
| Empty message | Table is **omitted** when `written_attaches` is empty (`has_attaches` false). MS-PST: table is optional; exists only if ≥1 Attachment object. |
| Store template | NBT `0x671` remains a **zero-row** TC (0069). Do not apply Strategy A to the template. |
| Columns | `ATTACHMENT_TABLE_COLUMNS` (6): `0x0E20` size, `0x3704` filename StringRef, `0x3705` method, `0x370B` rendering (`0xFFFFFFFF`), `0x67F2` LtpRowId (= attach NID), `0x67F3` LtpRowVer. All width 4. |
| Row width | `build_template_tc_columns`: 6×4 + bitmap `ceil(6/8)=1` → **`row_width = 25`**. `RowsPerBlock = Floor(8176/25) = 327`. Full leaf = 327×25 = **8175** + **1** byte dead space. Re-verify at execute (0100 taught 145 vs 146). |
| Overflow | `HeapBuilder::try_alloc` → `WriterError::Layout("heap page overflow…")`. Bubbles out of `build_message_pc` (`?`) — **hard fail of the write**, not a per-attach soft-fail and not a `known_gap`. |
| Recipient A | `build_recipient_table_strategy_a` already: `PagedHeapBuilder`, `alloc_tc_value` (cell NID if `> MAX_HEAP_VALUE_SIZE` 2048), `write_row_matrix_tree`, matrix `push` + `add_subnode_leaf` (0103 sort), empty `hnidRows=0` / `bid_sub=0`. |
| Paged HN | `lib.rs` `PagedHeapBuilder` (~1111): documented “recipient-table node only.” HNBITMAPHDR pages 8/136/264 fail closed (`D-0100-hn-bitmap-hdr`). Error string still says “recipient TC heap”. |
| Reader table | `pst-reader` `ltp/tc.rs` `load_from_table_bids` + `load_with_resolver` (0100): `hnidRows` NID, RowsPerBlock, cell HNID. **Embedded** `list_attachments_via_attach_table` already uses this. |
| Reader list | `PstFile::list_attachments` (~449–478) enumerates **`NidType::Attachment` (0x05) PCs**. It does **not** read `0x671`. That is why a missing/overflowed table does not fail current `list_attachments` asserts. |
| Existing tests | `writer_fidelity.rs` `per_message_attachment_table_rows_and_row_index` (~1056): **one** short-name attach; `read_subnode_data` + **`TableContext::load` (no resolver)**. Template test `attachment_table_template_present_empty_at_0x671` stays HID-only. |
| MessageSize | `message_size_uses_real_attachment_table_size` (~1445): body-only vs body+attach; table heap length is counted. After this track, also count matrix + cell extra bytes (same as recipient `extra_content_bytes`). |

**Why ~100+ overflows today.** Per row on one heap: filename UTF-16 (~40 B for a 20-char name) + 25 B matrix + RowIndex record + pagemap. 8176 / ~73 ≈ **110** modest-name rows. Moving the matrix off-heap is necessary; remaining filename HIDs still need **multi-page HN** (200 × 40 B ≈ 8 KiB).

### 2.3 MS-PST research (plan-time; re-verify at execute)

Fetched 2026-08-28:

| Source | What it says | Consequence |
|---|---|---|
| [MS-PST] Attachment Objects (Learn, `46eb4828…`; published rev **v11.2 / 2025-02-18** on the PDF TOC) | Table is **optional**; present iff ≥1 Attachment object. Locate by scanning the message subnode BTree for `NID_ATTACHMENT_TABLE`. **At most one** table. Attachment object subnode count **MUST match** table row count. | Do **not** emit empty `0x671` on messages with zero written attaches. Fail closed if Strategy A cannot store all `written_attaches` rows. |
| [MS-PST] TC / Row Matrix §2.3.4 / §2.3.4.4 (same family as 0100) | Typical TC: row matrix in a **subnode**; `RowsPerBlock = Floor((sizeof(block) – sizeof(BLOCKTRAILER)) / rgib[TCI_bm])`; rows must not span blocks; readers ignore dead space. Variable cells: HID on HN or NID in the 4-byte slot. | Reuse `write_row_matrix_tree`. Non-empty attach tables: `hnidRows` = NID, table `bid_sub` set. |
| [MS-PST] HID / HN §2.2.2.1 / §2.3.1.6 | `hidIndex` 11-bit **per page**; HNHDR page 0, HNPAGEHDR continuations; HNBITMAPHDR at pages **8, 136, 264, …** | Reuse `PagedHeapBuilder`. Keep bitmap-page fail-closed (`D-0100-hn-bitmap-hdr`). |
| [MS-PST] **Attachment Table Template** §2.4.6.1.1 (`47c336f7-2d9b-4f22-91c7-5bb422aaebbb`, updated 2024-11-12) | Each PST MUST have one template at `NID_ATTACHMENT_TABLE` (`0x671`); **MUST have no data rows**; MUST columns: `0x0E20` size, `0x3704` filename, `0x3705` method, `0x370B` rendering, `0x67F2` LtpRowId, `0x67F3` LtpRowVer — **exactly** `ATTACHMENT_TABLE_COLUMNS`. | 0069 six-column lock is spec-MUST, not a guess. Truncated `47c336f7` 404s; use the full GUID / toc.json. |
| [MS-PST] **Message Object Attachment Tables** §2.4.6.1.2 (`db45c8ae-6d38-4ab7-b444-a5cca3010101`) | Actual message tables contain the template columns **plus a number of extra properties**. | Extra columns are optional. Adding them is **out of scope** (lock 11). Six columns meet the MUST set. |
| [MS-PST] **Relationship** §2.4.6.3 (`f3fcc68c-53ee-4c2a-82d7-113e44f1fb3f`, Figure 12) | Rows map to attach-object subnodes via **RowIndex** (key = subnode NID). | Keep RowIndex BTH key = attach NID. Table row count MUST match written attach objects. |

**N/A this track:** crate-registry API churn (no new deps). Schema / matter-core version (writer-only). BCC. Recipient TC.

**Not independently verified here:** Outlook’s exact table UI for 328-row messages. CI proves on-disk row count + filename round-trip + `bid_sub`, not COM Outlook.

### 2.4 Why `list_attachments` is not proof

`PstFile::list_attachments` walks type `0x05` PCs. A unique-pst whose attach objects exist but whose `0x671` heap overflowed **never reaches disk** today (hard fail). After a naive “keep writing objects, skip the table” that list would still look complete while Outlook’s table is empty — **forbidden**. DoD tests **must** `list_subnode_entries` on the message, find `NidType::AttachmentTable`, and `load_from_table_bids(heap, bid_sub)` so `row_count` equals written attach objects and filenames resolve (HID or cell NID). Copy the `recipient_table_subnode` helper pattern (`writer_fidelity.rs` ~2609).

Do **not** rewrite top-level `list_attachments` to prefer the table (identity hashing uses attach PCs; 0086 strict path stays PC-based). Embedded `list_attachments_via_attach_table` already uses `load_from_table_bids` — keep it.

### 2.5 Tools (plan-time)

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3854 pinned).
- `ai-brains sync query` / `recall "attachment-table TC page overflow unique-pst"` — 0093 residual; 0100 parked attach writer; 0103 Completed; next free ID was **0104**.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` 0 pending / 0 unaudited drift. `scan --impact` **LOW** (HEAD `4becdfe`; dirty tree is skills + `agy-review.md` + `fixtures/keep_set_summary.json`, not product crates). Hotspot `export_exit_0078.rs` is out of scope.
- Ledger tx for this planning pass: `41cb5738-349d-4233-8ac7-90ffcbb5b719`.

### 2.6 ai-brains decisions absorbed

| Memory | Use here |
|---|---|
| 0100: Strategy A recipient; attach-table writer **out**; HNBITMAPHDR fail-closed | This track takes the parked attach writer only. Keep bitmap fail-closed. |
| 0103 Completed; Series P 0099–0103; next free **0104** unused; frontend **0105+** | **0104 is this track**, unique-pst, not frontend. |
| 0093: residual `D-0093-attachment-tc-page` | Close on DoD. |
| 0082 BCC opt-in | Unchanged. |

### 2.7 How this advances the north star

Counsel-facing unique-PST must be honest. Attach **objects** already write (0069/0070). The table is how Outlook names them. A heap overflow that aborts a volume (or a table that lists fewer rows than objects) is a silent fidelity hole of the same class 0100 closed for recipients.

### 2.8 Last-PR Cursor comments (merged #97, #96, #95, #94)

Skill: last 2–4 merged product PRs.

| PR | Comment | Verdict |
|---|---|---|
| **#97** (0103 docs) | No review / issue / inline comments. Bugbot n/a (docs). | n/a |
| **#96** (0103 SLBLOCK) | No review / issue / inline comments. Cursor Bugbot **pass** (no findings). | n/a — 0103 already Completed |
| **#95** (0102 docs) | No review / issue / inline comments | n/a |
| **#94** (0102 oracle) | No review / issue / inline comments | n/a |
| **#90** Bugbot (origin of 0103; not in last-four window) | `table_subs.insert(0, matrix_nid)` unsorted | **0103, closed.** Not this track. |

Nothing else to mint. No BCC-default track. Frontend stays **0105+**. **0104** was the unused Series P slot; this deferred promotion is the legitimate use (not a steal for Hermes).

### 2.9 Dual-AI review disposition (2026-08-28)

Reviews: `opencode-review.md` (Ready; no blocker/major) and `agy-review.md` (PASS). Neither asked to reopen BCC, recipient TC, HNBITMAPHDR, column extras, or `list_attachments` rewrite.

| Id | Source | Severity | Disposition | Spec landing |
|---|---|---|---|---|
| opencode-m1 | opencode-review.md | Minor | **Agree — fold** | `heap_data_len` has **no other consumer** (only `build_attachment_table_tc` return). Delete **both** or `-D warnings` fires. `table_len` = `built.heap.len()` (recipient pattern). Plan Phase 1 / DoD-1. |
| opencode-m2 | opencode-review.md | Minor | **Agree — fold** | `utf16le_bytes` is 2 B/char, no NUL. 200 × ~11-char names ≈ 4.4 KiB + BTH ≈ 1.6 KiB + TCINFO fits **one** 8176 page, so `heap > 8176` can fail a correct impl. Pin names to **≥20 BMP chars** (e.g. `attach_filename_test_{i:04}.txt`). Do **not** weaken to `>= 8176`. DoD-2b / §10.2 / plan Phase 2. |
| opencode-O1 | opencode-review.md | Opportunity | **Agree — partial** | §2.3: template leaf lives at full GUID `47c336f7-2d9b-4f22-91c7-5bb422aaebbb` (toc.json). Extra props optional (`db45c8ae`). DoD-3 one sentence. Truncated GUID 404s. |
| opencode-O2 | opencode-review.md | Opportunity | **Agree — fold** | Plan Phase 1: extend `add_subnode_leaf` doc-comment with “attachment-table cells+matrix (0104)”. |
| opencode-O3 | opencode-review.md | Opportunity | **Already covered** | 340-entry SLBLOCK vs HNBITMAPHDR fail-closed; no silent drop. |
| opencode-O4 | opencode-review.md | Opportunity | **Already covered** | Message `bid_sub`; 1-row test switch; MessageSize inequality. |
| opencode-O5 | opencode-review.md | Opportunity | **Already covered** | 25 / 327 / 1025-char divert. |
| agy-0104-1 | agy-review.md | — | **Already covered** | Strategy A helper + own `sub_counter`. |
| agy-0104-2 | agy-review.md | — | **Already covered** | row_width 25 / RowsPerBlock 327. |
| agy-0104-3 | agy-review.md | — | **Agree — partial** | Table-first DoD is locked. Its `heap > 8176` on 200 rows is the m2 undercount — corrected via ≥20-char names. |
| agy-0104-4 | agy-review.md | — | **Already covered** | HNBITMAPHDR error string → “TC heap”. |

**Declined / not locked**

- Treating six columns as a spec violation (agy exec “all PST tables” is overclaim — folder TCs stay as-is). Extra columns stay OOS.
- Weakening DoD-2b to `heap >= 8176`.
- Adding MAPI attach-table extras (`PR_ATTACH_NUM` / `PR_RECORD_KEY`).

---

## 3. In scope

1. Per-message attachment-table builder: Strategy A for **all `written_attaches` rows**. Reuse `PagedHeapBuilder`, `alloc_tc_value`, `write_row_matrix_tree`, `add_subnode_leaf`. Do not invent a second heap type.
2. Non-empty tables: row matrix as a **subnode** (`hnidRows` = NID, RowsPerBlock packing); table node `bid_sub` non-zero. Message-level `subnode_entries` push `(NID_ATTACHMENT_TABLE, table_bid, table_bid_sub)` — **not** `0`.
3. Filename bytes `> MAX_HEAP_VALUE_SIZE` (2048) → cell NID on the **table** SLBLOCK (same HID-vs-NID rule as 0100). Ordinary names stay HIDs on the paged heap.
4. Multi-page HN on the **attachment-table node** (HNHDR + HNPAGEHDR). Generalize `PagedHeapBuilder` docs + HNBITMAPHDR error string from “recipient TC” to **“TC heap”** so `D-0100-hn-bitmap-hdr` covers both. **Do not implement** HNBITMAPHDR.
5. Fail closed if A cannot store every written-attach row (page-2047, bitmap page, row width 0, `add_subnode_leaf` 340-entry ceiling). **No** `ATTACH_TC_TRUNCATED` event / `known_gap`.
6. Tests that load `0x671` via `load_from_table_bids` (see §10.2). Keep existing 1-row filename/RowIndex asserts after switching off HID-only `TableContext::load`.
7. Docs: `docs/unique-pst-export.md`, `docs/pst-writer-fidelity-v1.md` attachment-table row, CHANGELOG. Close `D-0093-attachment-tc-page` on implement.

## 4. Out of scope (do NOT do here)

- Recipient TC / BCC default (**0100 / 0082**). `--include-bcc-recipients` remains opt-in.
- Implementing HNBITMAPHDR (`D-0100-hn-bitmap-hdr`) — fail closed remains.
- Changing `ATTACHMENT_TABLE_COLUMNS` (do not add MAPI attach-table extras; do not move LtpRowId to `ib=0` — existing 0069 layout).
- Store-template `0x671` (stays zero-row, single-page, `TableContext::load` HID-only).
- Emitting an empty per-message `0x671` when there are zero written attaches (MS-PST optional).
- Rewriting `PstFile::list_attachments` to walk the table instead of type `0x05` PCs.
- Soft-fail policy for missing attach **payloads** (0069) — table rows remain **successfully written** attaches only (objects MUST match rows).
- `parents_only` emptying the attach list (still no table).
- Cloud named-prop write / PermissionType (**0092 / 0096**).
- Nested-depth flag (**0101**), oracle attest (**0102**), SLBLOCK emit-sort algorithm (**0103** — reuse as-is).
- Frontend / Hermes Series O (**0105+**).
- COM Outlook automation; client PSTs in git; in-tool ScanPST / CRC repair.

## 5. Preconditions & dependencies

- **P1 (blocking):** 0100 helpers on `main`: `PagedHeapBuilder`, `HeapTryAlloc`, `alloc_tc_value`, `write_row_matrix_tree`, `load_from_table_bids`. Verified @ `4becdfe`.
- **P2:** 0103 `add_subnode_leaf` sorts NID-ascending and fail-closes duplicates. Table cell NIDs + matrix must go through it (trailing **push**, never `insert(0)`).
- *Verified to date:* `bid_sub = 0` at `production.rs` ~3767; `build_attachment_table_tc` still single-page; 1-row test still `TableContext::load` without resolver.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Implementer “fixes” `list_attachments` (0x05 enum) only | DoD-2 requires `load_from_table_bids` on `0x671`. PC enum is necessary but **not sufficient**. |
| Existing 1-row test keeps `TableContext::load` | After `hnidRows` is a NID, HID-only load misses rows or errors. **Must** switch that test to `load_from_table_bids`. Template test stays `TableContext::load`. |
| Naive `write_data_chain` on a flat 328×25 matrix splits a row | Reuse `write_row_matrix_tree` (already unit-tested at width 100). DoD-2 328-row fixture. |
| Sharing `attach_nid_counter` (type 0x05) with table cell NIDs | Table uses its **own** `next_subnode_nid` counter (type `0x1F`), like recipient `sub_counter`. Cell NIDs live under the **table** node’s SLBLOCK, not the message’s attach list. |
| HNBITMAPHDR at ~8 pages of filename HIDs | Fail closed; document. ~170 short names/page × 8 ≈ 1360 attaches before the residual. Do not implement bitmap pages. |
| MessageSize under-count | Count table heap + `extra_content_bytes` (matrix + diverted filename bytes), same as recipient. Relative body vs body+attach test stays strictly larger. |
| Outlook still rejects for another reason | CI proves table row_count + filenames. Optional HITL Outlook open. |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Strategy A emit:** Per-message attachment table no longer uses single-page `HeapBuilder` for non-empty tables. **Delete** `build_attachment_table_tc` **and** `heap_data_len` (the latter has no other consumer; keeping either fails `-D warnings`). Row matrix is a subnode (`hnidRows` NID). Table `bid_sub` is **non-zero** when rows exist. Message push uses that `bid_sub`, not `0`. `table_len` = `built.heap.len()`. Empty messages still **omit** `0x671`. Store template `0x671` unchanged (zero rows). Fail closed if not all `written_attaches` rows can be stored. No `ATTACH_TC_TRUNCATED` production event.
- [ ] **DoD-2 — Tests:** (a) existing 1-row test still checks RowIndex / size / method / filename, but loads via `load_from_table_bids`; `hnidRows` nidType ≠ 0; `bid_sub` non-null; (b) **200** by-value attaches with **≥20 BMP-char** distinct names (e.g. `attach_filename_test_{i:04}.txt`; **not** `file_000.txt`): write **succeeds**; `list_attachments` len **200**; table `row_count == 200`; heap **> 8176** (multi-page HN — do not weaken to `>=`); (c) **328** short-name attaches (`> RowsPerBlock` 327): `row_count == 328`; matrix spans leaves; (d) one attach with **1025-char** filename: string round-trips; table SLBLOCK NIDs strictly increasing (`cEntries ≥ 2`: matrix + filename cell); (e) zero-attach message: **no** `0x671` subnode. No client PSTs in git.
- [ ] **DoD-3 — Docs:** `docs/unique-pst-export.md` 0100 residual sentence no longer lists attach-table as open; new attach Strategy A paragraph (row_width **25** / RowsPerBlock **327**, re-verify). Note the six columns are the MS-PST template **MUST** set; extra message-table properties are optional and not added. `docs/pst-writer-fidelity-v1.md` attachment-table row. CHANGELOG one-liner. `D-0093-attachment-tc-page` **closed / 0104**. `D-0100-hn-bitmap-hdr` stays open (error string may now say “TC heap”).
- [ ] **DoD-4 — Recorded:** `review.md`; registry **Completed**; ledger commit (`FEATURE` on `crates/pst-writer` at implement). No HITL required.

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
$env:CARGO_TARGET_DIR = 'C:\dev\Dedupe\target'
cargo test -p pst-writer per_message_attachment_table
cargo test -p pst-writer attachment_table
cargo test -p pst-writer message_size_uses_real_attachment_table_size
cargo fmt --all --check
cargo clippy -p pst-writer --all-targets -- -D warnings
# before implement-track publish:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Filter names re-verify at execute (new tests will be named in Phase 2). No operator INC* command. No unique-pst binary run required for DoD.

## 9. Deferred roll (mandatory)

Entire `docs/deferred.md` scanned 2026-08-28. Related open rows:

| Row | Disposition |
|---|---|
| **D-0093-attachment-tc-page** | **Absorb and close** on implement. This track. |
| **D-0100-hn-bitmap-hdr** | **Decline to implement.** Reuse fail-closed; generalize error string so attach heaps are covered. Stay residual. |
| **D-0100-slblock-nid-order** | **Already closed in 0103.** Reuse emit-sort; do not re-open. |
| **D-0093-recipient-tc-multipage** | **Already closed in 0100.** Do not touch recipient builder except shared helper comments. |
| **D-0094-inc-resmoke** | **Decline.** Optional operator HITL. INC* attach-table cloud providers were 0; large attach lists on INC* unknown. |
| **D-0097-window-edge-normalize** | **Decline.** Not Series P. |
| **D-0088-usgovcloud-microsoft-tld** | **Decline.** |
| **D-0067-embedded-depth** | **Decline.** 0101 narrowed the CLI half. |
| **D-0077-poly-fingerprint** / **D-0099-attach-crc-job-level** | **Decline.** |
| **D-0079-reader-buffer** | **Decline.** pst-reader buffer polish. |
| **D-0062-codesign** | **Decline.** Release ops. |
| Other `docs/deferred.md` rows | **Decline** — not attachment-table TC paging. |

Med/high never parked here. No BCC-default track. Frontend **0105+**.

## 10. Product locks (do not reopen)

1. Never mutate source PST / Purview files.
2. Never commit client PSTs, `output/`, `evidence/`, or matter folders with client mail.
3. No `unwrap` / `expect` in production.
4. Crate boundary: writer emit in `pst-writer`. Reader already has `load_from_table_bids` — do not change `list_attachments` 0x05 enumeration. Do not teach `dedup-engine` attach-table policy.
5. Unique-export: no silent attach/count drops. Table row count **equals** written attach objects. This track does **not** add `known_gap`.
6. No in-tool ScanPST / CRC repair of evidence.
7. `--include-bcc-recipients` default **off**.
8. Do not implement HNBITMAPHDR.
9. Do not raise `MAX_HEAP_VALUE_SIZE` or restore 3580 on message-PC.
10. Do not emit empty per-message `0x671`.
11. Do not change the six-column schema.
12. Table cell NIDs: **push** onto the table’s `table_subs`, then `add_subnode_leaf` (0103 sort). Never `insert(0)`.

### 10.1 Locked fix (closed)

**Option: reuse 0100 Strategy A on the attachment table.**

1. Replace the `HeapBuilder` + `build_attachment_table_tc` + `bid_sub = 0` call in `build_message_pc` with a `build_attachment_table_strategy_a(layout, rows) -> AttachmentTableBuilt { heap, table_bid_sub, extra_content_bytes }` (name may vary).
2. Inside: `PagedHeapBuilder::new(0xBC)`; per-row `alloc_tc_value` for filename; RowIndex BTH on the paged heap; TCINFO; if `rows.is_empty()` this helper is **not called** (caller already omits the table).
3. `layout.write_row_matrix_tree(&row_matrix, row_width)`; `matrix_nid = next_subnode_nid`; `table_subs.push`; patch `hnidRows`; `add_subnode_leaf(&table_subs)`.
4. Message: `subnode_entries.push((NID_ATTACHMENT_TABLE, table_bid, recip-style table_bid_sub))`; add `extra_content_bytes` into `written_content_bytes`.

**Declined:** keep matrix on heap and only page the HN — 328×25 = 8200 already exceeds one page; 0100 already proved matrix-as-subnode is the MS-PST typical layout.

**Declined:** cap/truncate attach-table rows with a ledger event. Fail closed or write all; no third path.

**Declined:** rewrite `list_attachments` to table-first.

### 10.2 Tests (minimum)

| Test | Assert |
|---|---|
| `per_message_attachment_table_rows_and_row_index` (extend) | Keep RowIndex = attach NID, size, method, filename. **Switch** to `attachment_table_subnode` + `load_from_table_bids`. `bid_sub` non-null; `hnidRows` nidType ≠ 0; `row_count == 1`. |
| New `attachment_tc_many_rows_round_trips` | **200** attaches, **≥20 BMP-char** distinct names (`attach_filename_test_{i:04}.txt`), tiny by-value payloads. Write succeeds. `list_attachments` len 200. Table `row_count == 200`. Heap `len > 8176` (do **not** use 11-char names; do **not** weaken to `>= 8176`). Every filename via `get_row_string`. |
| New `attachment_tc_matrix_spans_rows_per_block` | **328** attaches (`Floor(8176/25)+1`). `row_count == 328`. (Plan-time 327; re-verify width at execute.) |
| New `attachment_tc_long_filename_cell_nid` | One attach, display filename **1025** BMP chars. Round-trip via table `get_row_string`. Table `bid_sub` SLBLOCK `list_subnode_entries`: NIDs strictly increasing; **`len >= 2`**. |
| Existing `attachment_table_template_present_empty_at_0x671` | Unchanged HID `TableContext::load`; 0 rows; 6 columns. |
| Existing zero-attach messages | No `NidType::AttachmentTable` on the message subnode list. |
| Existing `message_size_uses_real_attachment_table_size` | Still strictly larger with an attach; counts real table + matrix bytes. |

Do **not** invert 0100 recipient tests. Do not add a 328-row test that also uses 1025-char names (HNBITMAPHDR / time). Tiny payloads (`b"x"`) so the test is table-bound, not XBLOCK-bound.

### 10.3 Arithmetic (plan-time; re-verify)

```
row_width = 6*4 + ceil(6/8) = 25
RowsPerBlock = Floor(8176/25) = 327
dead space on a full leaf = 8176 - 327*25 = 1 byte
```

If execute measures a different `table.row_size()`, recompute RowsPerBlock and the 328-row fixture **before** writing tests. Do not copy 0100’s 146.
