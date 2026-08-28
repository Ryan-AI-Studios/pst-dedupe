# 0103 — Recipient TC SLBLOCK NID Order

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open RowsPerBlock, HNBITMAPHDR,
> attach-table TC, or BCC default during implementation.

- **Track ID:** 0103-RecipientTcSlblockNidOrder
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `docs/unique-pst-export.md` + this track. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-28); do **not** chase it at execute.
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0100 (Completed). 0101 / 0102 Completed are not code dependencies; Series P order is 0099 → 0100 → 0101 → 0102 → **0103**.
- **Spec authored:** 2026-08-28
- **Series:** P (Unique-PST defensibility)
>
> **Closes:** `D-0100-slblock-nid-order`.
> **HITL:** none required. Optional operator Outlook open of a synthetic unique-pst with a >2048-byte UTF-16 display name is evidence, not CI (`D-0094-inc-resmoke` stays operator).
>
> **Last-PR fold-in (2026-08-28):** PRs **#95, #94, #93, #92**. Origin Bugbot is **#90** (already this track). Disposition in §2.8.
>
> **Review fold-in (2026-08-28):** `opencode-review.md` + `agy-review.md`. Disposition in §2.9 and `foldin-note.md`. `seven_bit` mirror arithmetic + exact SLBLOCK counts; Phase 2a preamble; Learn/PDF refresh note; emit-sort doc-comment hedge + two-counter note.
>
> Minted 2026-08-27 from PR **#90** Bugbot while planning **0101** (not stolen). Re-confirmed while planning **0102**. Expand complete this pass.

---

## 1. Objective

Emit recipient-table SLBLOCK entries in **NID-ascending** order so Outlook’s subnode BTree search can find the row-matrix NID and any cell-value NIDs.

Today `build_recipient_table_strategy_a` allocates cell NIDs monotonically, then `table_subs.insert(0, (matrix_nid, …))` puts a **later** matrix NID at the front. `Layout::add_subnode_leaf` writes that vec in order with **no sort**. `pst-reader` linear-scans SLBLOCK, so existing round-trip tests stay green while Outlook can miss long recipient strings.

This advances unique-export **defensibility**: a unique PST whose Display* names a recipient that the native table cannot resolve is not affidavit-clean.

---

## 2. Context (read before starting)

### 2.1 Diagnosis (PR #90 Bugbot, still live)

**Origin:** PR **#90** (0100) Cursor Bugbot, commit `d74e31dc` (merged `ab1c7b0`). Minted as this placeholder while planning **0101**; re-confirmed while planning **0102**. Not stolen into 0101 or 0102.

Bugbot text (verbatim gist): inserting the row-matrix NID at the front of `table_subs` leaves the table SLBLOCK unsorted once any cell NID exists. `next_subnode_nid` is already monotonic, so a **trailing push** would stay ordered. Outlook searches SLBLOCK by NID; an unsorted leaf can miss the matrix or cell values, so long recipient strings fail to resolve and a table with two or more cell NIDs can appear empty.

### 2.2 Live code snapshot (verified 2026-08-28, `main` @ `8e0e434`)

Re-verify line numbers at execute.

| Surface | State |
|---|---|
| Cell divert | `production.rs` `alloc_tc_value` (~4438): bytes `> MAX_HEAP_VALUE_SIZE` (2048) → `next_subnode_nid` + `table_subs.push`. HID otherwise. |
| Matrix last | `build_recipient_table_strategy_a` (~4708–4714): after the row loop, `matrix_nid = next_subnode_nid`, then **`table_subs.insert(0, (matrix_nid, matrix_bid, 0))`**, then `add_subnode_leaf(&table_subs)`. |
| Counter | `next_subnode_nid` (~3079): `counter += 1; (counter << 5) \| 0x1F` (LTP type so `Hid::hid_type() != 0`). Monotonic. |
| Emit | `Layout::add_subnode_leaf` (~5463): encodes `entries` in **call order**. No sort. Duplicate NIDs not checked. Fail-closed only if payload `> MAX_BLOCK_DATA` (~340 SLENTRYs). |
| Empty TC | `hnidRows = 0`, `bid_sub = 0` — no SLBLOCK. Unchanged. |
| Reader lookup | `pst-reader` `ndb/block.rs` `read_subnode_data_at` (~364) and `find_subnode_entry` (~590): **linear** scan / `find`. Order-blind. |
| Reader cells | `get_row_string` resolves NID via `cell_subnodes` map keyed by HNID — also order-blind. |
| Existing test | `writer_fidelity.rs` `recipient_tc_long_string_cell_nid_round_trips` (~2858): 1025-char display, short email/smtp. **`list_recipients` only**. Does **not** assert on-disk SLBLOCK order. That is why HEAD is green. |
| `seven_bit` mirror | `seven_bit = display` when display is non-empty (~4504), then its own `alloc_tc_value` (~4540). A 1025-char display therefore diverts **twice** (DisplayName + 7bit). RecordKey is 16 B, EntryId 24 B, SearchKey ASCII `TYPE:ADDR` — all HID at this size. |
| Other `insert(0)` | Only this `table_subs.insert(0, …)` in `pst-writer`. Message-level `subnode_entries` **push** body/attach/0x671/0x692 in allocation order (mixed nidTypes). |

**HEAD on-disk bug is already two-cell.** The existing long-display fixture’s SLBLOCK is `[matrix_high, display_cell, seven_bit_cell]` = **3** entries (matrix NID highest, prepended). Not “matrix + one display cell.”

Why INC* often never hits this: short Display/SMTP strings stay HIDs (`MAX_HEAP_VALUE_SIZE` 2048). Cell NIDs appear only when a per-row variable value exceeds 2048 **bytes** (UTF-16 → **1025** BMP chars). The 136-row short-name class is HID-only; SLBLOCK is then a single matrix entry and is trivially sorted.

### 2.3 MS-PST research (plan-time; re-verify at execute)

Fetched 2026-08-28:

| Source | What it says | Consequence |
|---|---|---|
| [MS-PST] PDF v20220215 §2.2.2.8.3.3 (opencode: published rev **v11.2 / 2025-02-18**) | Subnode BTree = SIBLOCK + SLBLOCK / SIENTRY + SLENTRY. SLENTRY.nid is unique **within the parent node**. | Leaf keys are NIDs. Semantic content of these sections unchanged vs v20220215. |
| Same PDF §2.2.2.7.7.2 BTENTRY | Child page keys are ≥ the parent key. NBT/BBT are searched by key. | Subnode BTree is the same family; Outlook is expected to **search by NID**, not scan. |
| `ARCHITECTURE.md` SLBLOCK | Unicode SLENTRY = `nid(8)+bidData(8)+bidSub(8)`; `btype=0x02`, `cLevel=0x00`. | On-disk layout our writer already emits. |
| Learn HTML for SLBLOCK | Plan-time leaf URLs **404**’d. Opencode reports leaves live via `toc.json` GUIDs (`0c7d9bd5` Subnode BTree, `5182eb24` SLBLOCK, `85c4d943` SLENTRY, `bc8052a3` BTENTRY). Fold-in re-fetch of a guessed full GUID still 404. | **Re-fetch via toc.json at execute.** SLBLOCK HTML itself does **not** say `rgentries MUST be sorted` — that is a BTree-family inference. Keep the hedge in the `add_subnode_leaf` doc-comment. |

**N/A this track:** crate-registry API churn (no new deps). Schema / matter-core version (writer-only).

**Not independently verified here:** Outlook’s exact binary-search implementation. Bugbot + BTree key rules are the working model. CI proves **on-disk NID order**, not COM Outlook.

### 2.4 Why reader tests do not catch this

`pst-reader` walks every SLENTRY. Unsorted `[matrix_high, display_cell, seven_bit_cell, …]` still round-trips through `list_recipients`. DoD tests **must parse the recipient-table `bid_sub` SLBLOCK** (via `list_subnode_entries` or raw bytes) and assert NIDs are **strictly increasing**. Do not treat `list_recipients` alone as proof.

### 2.5 Tools (plan-time)

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3851 pinned).
- `ai-brains sync query` / `recall "SLBLOCK NID sort Outlook subnode"` — 0101 minted this track for #90; 0100 Strategy A locks stay; do not steal into 0101/0102.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` 0 pending / 0 unaudited drift. `scan --impact` **LOW** (HEAD `8e0e434`; dirty tree is skills + `agy-review.md` + `fixtures/keep_set_summary.json`, not product crates). Hotspot `export_exit_0078.rs` is out of scope.
- Ledger tx for this planning pass: `52d24112-93b8-46f5-a423-b1a3f2de185e`.

### 2.6 ai-brains decisions absorbed

| Memory | Use here |
|---|---|
| 0101: 0103 minted for #90 SLBLOCK NID order; not stolen into depth | This track. Do not touch `--max-embedded-depth`. |
| 0100: Strategy A, BCC 0082, attach-table / HNBITMAPHDR out | Stay out. |
| 0102: #90 stays 0103 | Confirmed; 0102 Completed. |

### 2.7 How this advances the north star

Counsel-facing unique-PST must be honest. 0100 already writes every included TC row, including cell NIDs for long strings. If Outlook cannot **find** those subnodes, the table lies even though our reader says it is complete. Sorting the leaf is the missing 0100 emit invariant.

### 2.8 Last-PR Cursor comments (merged #95, #94, #93, #92)

Skill: last 2–4 merged product PRs. Also re-read origin **#90** because it **is** this track.

| PR | Comment | Verdict |
|---|---|---|
| **#95** (0102 docs) | No review / issue / inline comments | n/a |
| **#94** (0102 oracle) | No review / issue / inline comments | n/a |
| **#93** (0101 docs) | No review / issue / inline comments | n/a |
| **#92** (0101 depth) | No review / issue / inline comments | n/a |
| **#90** Bugbot (origin; not in the last-four window) | `table_subs.insert(0, matrix_nid)` unsorted once cell NIDs exist; Outlook searches by NID | **This track.** Diagnosis re-verified live @ `8e0e434` `production.rs` ~4711. |

Nothing to mint. No 0104. Frontend stays **0105+**. No BCC-default track.

### 2.9 Dual-AI review disposition (2026-08-28)

Reviews: `opencode-review.md` (Ready; no blocker/major) and `agy-review.md` (PASS). Neither asked to reopen BCC, attach-table TC, HNBITMAPHDR, matrix-NID hoist, or reader binary search.

| Id | Source | Severity | Disposition | Spec landing |
|---|---|---|---|---|
| opencode-m1 | opencode-review.md | Minor | **Agree — fold** | `seven_bit` mirrors display → existing fixture is already 2 cell NIDs + matrix = **3**. New long-display+email fixture with **short/absent smtp** = display + seven_bit + email + matrix = **4**. DoD-2 / §10.2 / plan 2b exact counts. |
| opencode-m2 | opencode-review.md | Minor | **Agree — fold** | plan Phase 2a: `add_subnode_leaf` has **no** existing unit test; copy `Layout::new()` + `layout.blocks` from `write_data_chain_*` only. |
| opencode-O1 | opencode-review.md | Opportunity | **Agree — partial** | §2.3: cite v11.2; Learn leaves via toc.json (re-verify at execute). Keep Outlook binary-search / “SLBLOCK never says MUST sort” hedge in the emit doc-comment. |
| opencode-O2 | opencode-review.md | Opportunity | **Already covered** + one example | Risk table + handoff. After emit-sort, attach NIDs (type 0x05) sort before same-index 0x1F body NIDs; `0x671`/`0x692` sort last. No index-order asserts. |
| opencode-O3 | opencode-review.md | Opportunity | **Agree — fold** | Phase 1 doc-comment: current callers use disjoint NID namespaces (two counters + fixed 0x671/0x692); duplicate fail-closed is defense, not a live green-test flip. |
| opencode-O4 | opencode-review.md | Opportunity | **Already covered** | Only `insert(0)` in writer; NBT/BBT already sort. |
| opencode-O5 | opencode-review.md | Opportunity | **Already covered** | 340-entry ceiling; sort is length-invariant. |
| opencode-O6 | opencode-review.md | Opportunity | **Already covered** | Deferred/registry/docs targets. |
| agy-0103-1 | agy-review.md | — | **Already covered** | Trailing push monotonicity. |
| agy-0103-2 | agy-review.md | — | **Already covered** | Emit-sort + duplicate `WriterError::Layout`. |
| agy-0103-3 | agy-review.md | — | **Agree — partial** | On-disk `list_subnode_entries` is required. Its “single cell NID / `>= 2`” framing is the m1 undercount — corrected to exact **3** / **4**. |
| agy-0103-4 | agy-review.md | — | **Already covered** | 1025-char UTF-16 divert; HID-only `cEntries == 1`. |

**Declined / not locked**

- Treating “SLENTRY keys MUST be sorted” as an explicit MS-PST SLBLOCK sentence (agy exec summary). Keep BTree-family inference + hedge.
- Long smtp on the dual-string fixture (would add a 5th SLENTRY). Lock smtp **short or `None`**.
- Rewriting `pst-reader` to binary-search.

---

## 3. In scope

1. Recipient-table builder: stop `insert(0)`; **push** the matrix entry so `next_subnode_nid` order is also vec order.
2. `Layout::add_subnode_leaf`: sort SLENTRY by NID **ascending** before encode; **fail closed** on duplicate NIDs (`WriterError::Layout`). This is the emit invariant for **every** SLBLOCK this function writes (recipient-table, message, attach). Do not separately rewrite message-level push order.
3. Tests that inspect **on-disk** SLBLOCK NID order (unit + fidelity). Keep existing `list_recipients` round-trips.
4. Docs: `docs/unique-pst-export.md` 0100 paragraph, `docs/pst-writer-fidelity-v1.md` recipient row, CHANGELOG. Close `D-0100-slblock-nid-order` on implement.

## 4. Out of scope (do NOT do here)

- RowsPerBlock / multi-block HN / empty-table `hnidRows = 0` (0100, done).
- HNBITMAPHDR (`D-0100-hn-bitmap-hdr`).
- Attachment-table TC writer (`D-0093-attachment-tc-page`).
- SIBLOCK multi-level (still fail-closed at ~340 leaf entries).
- Rewriting `pst-reader` to binary-search SLBLOCK (linear scan stays; it is not the product consumer).
- Allocating `matrix_nid` **before** the row loop (declined — extra state; push + emit-sort is enough).
- Changing `MAX_HEAP_VALUE_SIZE` (2048) or diverting ordinary short names to cell NIDs.
- BCC default (**0082**). `--include-bcc-recipients` remains opt-in.
- Oracle attest (**0102**, Completed). Nested depth (**0101**, Completed).
- Frontend / Hermes Series O (**0105+**). Do not steal **0100–0104**.
- COM Outlook automation; client PSTs in git; in-tool ScanPST / CRC repair.

## 5. Preconditions & dependencies

- **P1 (blocking):** 0100 Strategy A on `main` (`build_recipient_table_strategy_a`, cell NID divert, `add_subnode_leaf`). Verified @ `8e0e434`.
- **P2:** `pst-reader` `list_subnode_entries` / `find_subnode_entry` available to tests (already used by `recipient_table_subnode` in `writer_fidelity.rs`).
- *Verified to date:* `insert(0)` still present at `production.rs` ~4711; reader still linear; long-string fidelity test still does not assert SLBLOCK order.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Implementer “fixes” only `list_recipients` | DoD-2 requires `list_subnode_entries` on `bid_sub` (or raw SLENTRY parse). Reader round-trip is necessary but **not sufficient**. |
| Sorting in `add_subnode_leaf` changes message-level SLBLOCK order (mixed 0x1F / attach / 0x671 / 0x692) | **Intended.** Outlook searches those leaves too. Tests that `find` by `nid_type` stay valid; do not add index-order asserts. |
| Duplicate NID after sort | Fail closed `WriterError::Layout`. Unit test. |
| HID-only tables (INC* 136-row class) | Single matrix SLENTRY; still strictly sorted. Do not require a cell NID on that fixture. |
| Outlook still rejects for another reason | CI proves order. Optional HITL Outlook open. Do not claim ScanPST in this env. |
| Touching attach-table TC while sorting | Sort is emit-only; `build_attachment_table_tc` bytes stay identical. |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Emit order:** `table_subs.insert(0, …)` is **gone**. Matrix entry is **pushed**. `add_subnode_leaf` encodes SLENTRYs in **strictly increasing NID** order and returns `WriterError::Layout` on duplicate NIDs. Empty tables still `bid_sub = 0`.
- [ ] **DoD-2 — Tests:** (a) unit: unsorted input to `add_subnode_leaf` writes ascending NIDs; (b) unit: duplicate NID errors; (c) fidelity: existing long-display fixture — `seven_bit` also diverts → on-disk SLBLOCK **`cEntries == 3`**, strictly increasing, `hnidRows` present in the leaf; (d) fidelity: long display **and** long email, **smtp short or `None`** — display + seven_bit + email + matrix → **`cEntries == 4`**, strictly increasing, both strings round-trip via `list_recipients`. No client PSTs in git.
- [ ] **DoD-3 — Docs:** `docs/unique-pst-export.md` Strategy A paragraph and `docs/pst-writer-fidelity-v1.md` recipient row state that recipient-table (and `add_subnode_leaf`) SLBLOCK NIDs are ascending. CHANGELOG one-liner. `D-0100-slblock-nid-order` **closed / 0103**.
- [ ] **DoD-4 — Recorded:** `review.md`; registry **Completed**; ledger commit (`BUGFIX` on `crates/pst-writer` at implement). No HITL required.

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
$env:CARGO_TARGET_DIR = 'C:\dev\Dedupe\target'
cargo test -p pst-writer add_subnode_leaf
cargo test -p pst-writer recipient_tc
cargo fmt --all --check
cargo clippy -p pst-writer --all-targets -- -D warnings
# before implement-track publish:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

`--lib` filters re-verify at execute (`add_subnode_leaf_*` lives in `production.rs` `mod tests`; `recipient_tc_*` in `writer_fidelity.rs`).

No operator INC* command. No unique-pst binary run required for DoD.

## 9. Deferred roll (mandatory)

Entire `docs/deferred.md` scanned 2026-08-28. Related open rows:

| Row | Disposition |
|---|---|
| **D-0100-slblock-nid-order** | **Absorb and close** on implement. This track. |
| **D-0100-hn-bitmap-hdr** | **Decline.** Fail-closed if a heap would land on page 8/136/264. Not SLBLOCK order. |
| **D-0093-attachment-tc-page** | **Decline.** Attach-table writer stays single-page. Emit-sort does not change its heap bytes. |
| **D-0093-recipient-tc-multipage** | **Already closed in 0100.** This track is the residual emit invariant. |
| **D-0094-inc-resmoke** | **Decline.** Optional operator HITL; INC* short-name class often never allocates cell NIDs. |
| **D-0097-window-edge-normalize** | **Decline.** Parked polish. Not Series P. |
| **D-0088-usgovcloud-microsoft-tld** | **Decline.** |
| **D-0067-embedded-depth** | **Decline.** 0101 narrowed the CLI half. |
| **D-0077-poly-fingerprint** / **D-0099-attach-crc-job-level** | **Decline.** 0099/0102 residuals. |
| **D-0062-codesign** | **Decline.** Release ops. |
| Other `docs/deferred.md` rows | **Decline** — not recipient-table SLBLOCK order. |

Med/high never parked here. No BCC-default track. No frontend steal of 0100–0104.

## 10. Product locks (do not reopen)

1. Never mutate source PST / Purview files.
2. Never commit client PSTs, `output/`, `evidence/`, or matter folders with client mail.
3. No `unwrap` / `expect` in production.
4. Crate boundary: writer emit in `pst-writer`. Do not change `pst-reader` search algorithm. Do not teach `dedup-engine` SLBLOCK policy.
5. Unique-export: no silent recipient/attach/count drops. This track does not add `known_gap`.
6. No in-tool ScanPST / CRC repair of evidence.
7. `--include-bcc-recipients` default **off**.
8. Do not implement HNBITMAPHDR or attach-table Strategy A.
9. Do not raise `MAX_HEAP_VALUE_SIZE` or restore 3580 on message-PC.
10. Do not allocate matrix NID before the row loop as a substitute for sort.
11. `add_subnode_leaf` sort is **ascending NID**; fail closed on duplicates; keep the ~340 entry ceiling.

### 10.1 Locked fix (closed)

**Option: trailing push + emit-sort.**

1. In `build_recipient_table_strategy_a`, replace

   ```rust
   table_subs.insert(0, (matrix_nid, matrix_bid, 0));
   ```

   with

   ```rust
   table_subs.push((matrix_nid, matrix_bid, 0));
   ```

   `next_subnode_nid` stays monotonic. Matrix NID remains the highest (allocated last). Cell NIDs then matrix is already sorted **before** emit-sort.

2. In `Layout::add_subnode_leaf`, copy `entries`, `sort_by_key` on NID, reject adjacent equal NIDs, then encode. Callers keep their vecs. Unsorted callers (including a future `insert(0)`) still emit a legal leaf.

**Declined:** hoist `matrix_nid` before the row loop so `insert(0)` “looks” like matrix-first. Extra state; no Outlook benefit once the leaf is sorted.

**Declined:** path-aware / reader binary search as the fix. The product consumer is Outlook; the writer must emit a BTree-shaped leaf.

### 10.2 Tests (minimum)

| Test | Assert |
|---|---|
| `add_subnode_leaf_emits_nids_ascending` | Input `[(0x9F,…), (0x3F,…), (0x5F,…)]` → on-disk SLENTRY nids `0x3F, 0x5F, 0x9F`. `btype=0x02`, `cLevel=0x00`. |
| `add_subnode_leaf_duplicate_nid_errors` | Two entries with the same NID → `WriterError::Layout`. |
| `recipient_tc_long_string_cell_nid_round_trips` (extend) | Keep string round-trip. **Add:** `list_subnode_entries` on recip `bid_sub`; NIDs strictly increasing; **`len == 3`** (matrix + display cell + seven_bit cell); `hnid_rows` is in the leaf. |
| New `recipient_tc_two_cell_nids_slblock_sorted` | Display **and** email each 1025 chars; **smtp `None` or short** (do not also lengthen smtp). `list_recipients` both strings. SLBLOCK **`len == 4`**, strictly increasing. |

Do **not** invert 0100 all-rows / empty / BCC / RowsPerBlock tests.
