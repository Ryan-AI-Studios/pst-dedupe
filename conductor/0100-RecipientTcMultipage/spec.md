# 0100 — Recipient TC Multipage (Strategy A)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open the matrix during implementation.

- **Track ID:** 0100-RecipientTcMultipage
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → unique-PST recipient fidelity (file not present on this machine at plan-time; re-verify at execute)
- **Cross-repo contract:** n/a
- **Status:** In progress
- **Depends on:** 0082 · 0093 (Strategy **B** shipped) · 0098 · 0099 (all **Completed**)
- **Spec authored:** 2026-08-27
- **Series:** P (Unique-PST defensibility)
>
> **Promotes:** `D-0093-recipient-tc-multipage` (close on DoD).
> **Residual after this track:** `D-0093-attachment-tc-page` (attach-table TC); `D-0100-hn-bitmap-hdr` (HNBITMAPHDR pages 8/136/264 if a table ever needs them); message-PC 3580 restore (shared 0093 research — not wired here).
>
> **Review fold-in (2026-08-27):** `opencode-review.md` + `agy-review.md`. Disposition in §2.8. Phase 0 / product locks stay closed. Shared `TableContext::load` (four concat sites), empty `hnidRows = 0`, and a **>RowsPerBlock** matrix fixture are now DoD. Opencode's 154-row threshold was wrong. Live `row_width` **56** → `Floor(8176/56) = 146` exactly (plan-time 145 was off-by-one).

---

## 1. Objective

Write **every included recipient TC row** (To/Cc; BCC still 0082 opt-in) into the unique-PST native so Outlook/native review sees the same distribution list the source table had. Replace 0093 Strategy B single-page budget cap as the production path. Truncation of included rows is no longer a `known_gap`.

---

## 2. Context (read before starting)

### 2.1 Operator evidence (INC0102784, post-0098)

`output/inc0102784-post-0098/` (operator-local; not in git). Same inputs/order as 0097.

| Signal | Value |
|---|---|
| Written / verify found | **4055 / 4055** (0098) |
| Recipient TC | **39** messages, **3082** truncated rows, QC `known_gap` (`RECIPIENT_TC_TRUNCATED`) |
| Largest sampled class (0093) | source **136** vs kept **48** on one message |
| Display* | Full (message-PC diversion); the gap is **TC rows**, not DisplayTo/Cc strings |
| Exit (post-0099) | **64** `ATTACH_SOFT_FAIL` (depth → **0101**, not this track) |

Counsel cannot swear the unique PST: Display* names the full list; the per-message recipient table does not.

### 2.2 Live code snapshot (verified 2026-08-27, `main` @ `45c29de`)

| Surface | State |
|---|---|
| Strategy B | `production.rs` `build_recipient_table_budget_aware` — catch-and-retry from `RECIPIENT_TC_ROW_HINT` (48) down on `heap page overflow`; To→Cc→Bcc via `order_recipients_for_tc` |
| Row builder | `build_recipient_table_tc`: **7–8 `try_alloc`s per row** (display, addrType, email, 7bit, RecordKey, EntryId, SearchKey, optional SMTP) + **one HID for the whole row matrix** + TCINFO + RowIndex BTH |
| Heap | `HeapBuilder` is **single-page**; `try_alloc` HID is `index << 5` (`hidBlockIndex` always 0). Overflow → `WriterError::Layout` |
| Recip table node | `subnode_entries.push((NID_RECIPIENT_TABLE, bid, 0))` — **`bid_sub = 0`** |
| Contract | `fidelity_contract_v1.recipient_table` = **Preserved** (0082: “TC present & schema correct”). 0093 did **not** rewrite that to “all source rows” |
| QC | `unique_pst_qc.rs`: To/Cc set mismatch + matching truncate event + `kept_count` bind → `known_gap`; mismatch without event → `defect`. BCC omit still 0082 `known_gap` |
| Tests that **assert truncation** | `writer_fidelity.rs` `recipient_tc_budget_truncates_with_event_and_keeps_display` (140 rows, `include_bcc=true`); `recipient_tc_budget_keep_below_hint_with_long_names`; CLI `recipient_tc_truncate_event_is_known_gap_not_defect` |
| Reader TC | `load_tc` concatenates every SLENTRY when `bid_sub` non-null. **Same concat is duplicated** in `list_recipients` (`recipient.rs`), embedded attach table (`embedded.rs` ~612), embedded recipient table (`embedded.rs` ~784) — they call `TableContext::load` with pre-concatenated bytes and **never call `load_tc`**. `TableContext::load` counts `len / row_size` with **no RowsPerBlock**. `get_row_string` is HID-only (`heap.get`; `InvalidHid` if `hid_type != 0`) |
| Empty TC | `build_recipient_table_tc` `try_alloc(&row_matrix)` is **unconditional** — empty tables get a non-zero `hnidRows` HID today (contradicts lock 9) |
| Row width | `RECIPIENT_TABLE_COLUMNS` (15 cols): 13×4 + 2×1 + bitmap `ceil(15/8)=2` → **`row_width = 56`**. Execute: `RowsPerBlock = Floor(8176/56) = 146` exactly (plan-time **145** was wrong). 140×56 = 7840 **fits one leaf** — 140-row fixture does **not** prove matrix spanning. Full leaves at this width have **no dead space**; dead-space packing is proven with a non-dividing width in a writer unit test. |
| SLBLOCK | `add_subnode_leaf` single-SLBLOCK `(8176-8)/24 ≈ 340` entries; fail-closed if exceeded |
| Reader HN | `ltp/hn.rs` already decodes `hidBlockIndex` and HNPAGEHDR `ibHnpm` on continuation pages |

### 2.3 MS-PST research (plan-time; re-verify at execute)

Microsoft Learn [MS-PST] (fetched **2026-08-27**; PDF v20220215 used for §2.3.4.4 formulas):

| Rule | Spec | Consequence for 0100 |
|---|---|---|
| Typical TC layout | §2.3.4: row matrix **in a subnode**; small TCs may inline | Non-empty recipient TCs **always** use `hnidRows` = NID (empty → 0) |
| `hnidRows` | §2.3.4.1 TCINFO: HNID to the Row Matrix | Writer must set a real NID, not a heap HID, for non-empty tables |
| Integral rows per block | §2.3.4.4: `RowsPerBlock = Floor((sizeof(block) – sizeof(BLOCKTRAILER)) / rgib[TCI_bm])`; rows **must not span** blocks; non-last blocks **MUST** be 8192 bytes; readers **ignore dead space** | **Forbidden:** dump a flat `Vec` through `write_data_chain` if that splits a row. Dedicated row-matrix data tree |
| Variable cell values | §2.3.4.4.2 / §2.6.2.4.2: ≤3580 → HN HID; >3580 → subnode NID in the 4-byte slot. HID vs NID distinguished by `nidType` | Per-row strings stay HIDs on the TC heap unless they exceed this writer’s `MAX_HEAP_VALUE_SIZE` (2048, 0093 deviation) |
| Multi-block HN | §2.3.1.6 HNHDR / HNPAGEHDR / HNBITMAPHDR | Needed for per-row string HIDs (~8 × N) that will not fit one 8176 page even after the matrix leaves the heap. Reader already has `hidBlockIndex` |
| HID `hidIndex` | §2.2.2.1 / HID: **11 bits**, 1-based, **per page** (`hidBlockIndex` is 16 bits) | Current global `index << 5` is a single-page encoding. Multi-block must reset `hidIndex` per page (max 2047 allocs **per page**) |
| HNBITMAPHDR | pages **8, 136, 264, …** | INC* 136-row class is ~6 pages of string HIDs at ~40 B — likely **&lt; 8**. If a write would need a bitmap page, **fail closed** (`D-0100-hn-bitmap-hdr`) rather than emit a lying HNPAGEHDR |

**N/A this track:** crate-registry API churn (no new deps expected).

### 2.4 Why Strategy B still truncates after the matrix is “the problem”

Removing the matrix HID from the heap is **necessary but not sufficient**. 136 rows × 8 string HIDs ≈ 1088 allocations and tens of KB of UTF-16 — still **&gt; 8176**. 0100 therefore ships **both**:

1. Row-matrix subnode (MS-PST typical layout + RowsPerBlock packing).
2. Multi-block HN **on the recipient-table node data** for remaining HIDs (TCINFO, RowIndex BTH, per-row strings).

Do **not** divert every per-row string to a subnode as the primary path: current `load_tc` would concatenate those SLENTRYs into fake extra rows until the reader is fixed, and Outlook expects HID-typed cells for ordinary display names.

### 2.5 Product locks (closed)

1. **All included rows.** After 0100, `build_recipient_table_budget_aware` must not cap. Fail-closed `WriterError` if the table cannot be stored (HID page 2047, HNBITMAPHDR needed, row width 0, etc.). No silent 48-row fallback.
2. **BCC default unchanged (0082).** Filter BCC **before** the TC build unless `--include-bcc-recipients`. BCC omit is `known_gap` via the existing display_bcc / DroppedByDesign path — **not** `RECIPIENT_TC_TRUNCATED`.
3. **Display* stay full.** Message-PC diversion unchanged. Do not clip DisplayTo/Cc/Bcc to “make the TC fit.”
4. **Contract.** Keep `recipient_table` = Preserved (schema present). After 0100, To/Cc set mismatch **is a defect** unless it is BCC-filter explained. Do not mid-v1 invent `recipient_table_rows`.
5. **QC truncate branch.** Keep the 0093 matching-event → `known_gap` code for honesty if an event is ever injected. Production unique-pst **must not emit** `RECIPIENT_TC_TRUNCATED` for included rows. DoD fixture: **0** truncate counters.
6. **Reader is in scope at `TableContext::load`, not `load_tc` alone.** Unique-pst QC and nested-message reads round-trip through `pst-reader`. Four sites currently pre-concatenate SLENTRYs (`load_tc`, `list_recipients`, embedded attach table, embedded recipient table). The RowsPerBlock / `hnidRows`-NID / cell-HNID logic lives **inside** `TableContext::load` (pass `bid_sub` + a subnode resolver, or pre-resolved matrix bytes for **that NID only**). Do not concatenate sibling SLENTRYs. Attach-table **writer** stays byte-identical (`D-0093-attachment-tc-page`); sharing the reader load path is required so 0100 recipient tables round-trip from every site.
7. **Cell HNID.** If a per-row string is diverted to a subnode (size &gt; `MAX_HEAP_VALUE_SIZE`), TC string lookup must resolve that NID from the **table** subnode tree (same HID-vs-NID rule as PC). Ordinary names remain HIDs.
8. **HeapBuilder scope.** Multi-block HN is for **recipient TC node data** this track. Do **not** restore 3580 for message-PC helper strings, and do **not** change `build_attachment_table_tc` (`D-0093-attachment-tc-page`).
9. **Empty tables.** Zero-row TC still present at `0x692` (0082 MUST). `hnidRows = 0`, `hidRowIndex = 0`.
10. **No production `unwrap`/`expect`.** Synthetic fixtures in CI. INC* re-smoke is operator-local (`D-0094-inc-resmoke`).
11. **Sources stay read-only.** No in-tool ScanPST / heap repair of evidence.

### 2.6 Cursor / last-PR comments (mandatory scan)

Last 4 merged PRs: **#89** (0099), **#88** (0097), **#87** (0096), **#86** (0095). Issue comments empty on all four. Line comments:

| PR | Claim | Verdict |
|---|---|---|
| #89 Bugbot | `compare_integrity_counters` attest pointers `/export_risk/inputs/…` run after `normalize_summary_for_oracle`, which `strip_keys_recursive`s every key named `inputs` (`SUMMARY_ALLOWLIST_KEYS`). Attest fields never compare | **Valid, not 0100.** Live code still lists `"inputs"` in the allowlist (verified). **Mint 0102-ExportOracleInputsAttest** + `D-0099-oracle-inputs-attest`. Do not steal into this TC track |
| #88 Bugbot | `handle_window_edge_bare` checks `acc.seen` without `normalize_candidate` | **Valid, not 0100.** Park `D-0097-window-edge-normalize` (P3 polish on Completed 0097). Do not mint a Series P track |
| #87 / #86 | no review comments | — |

### 2.7 Tools notes (plan-time)

- ai-brains: used from `C:\dev\Dedupe` (preflight + sync query + recall). Recalled 0093 Strategy B lock + Series P placeholder order.
- ledgerful: doctor ready (warn: phantom promote, sig-pin, sig-version — pre-existing). Ledger 0 pending. Tx `2957492a` for this planning pass.
- `C:\dev\Dedupe-plan.md`: **not present** on this machine; cited anyway as board convention.

### 2.8 Dual-AI review disposition (2026-08-27)

Reviews: `opencode-review.md` and `agy-review.md`. Neither asked to reopen BCC default, attach-table writer, 3580 message-PC, or HNBITMAPHDR.

| Id | Source | Severity | Disposition | Spec landing |
|---|---|---|---|---|
| opencode-1 | opencode-review.md | Major | **Agree — fold** | Shared `TableContext::load`; four concat sites; lock 6; DoD-4; plan Phase 1 |
| opencode-2 | opencode-review.md | Major | **Agree — fold** | `get_row_string` HID-only is a real gap; same resolver as (1); lock 7 |
| opencode-3 | opencode-review.md | Major | **Agree — fold** | Empty `hnidRows = 0` explicit; DoD-3; plan Phase 2 |
| opencode-4 | opencode-review.md | Minor | **Agree — fold** | `MAX_HEAP_VALUE_SIZE` divert in recipient builder is **new**; plan Phase 2 bullet |
| opencode-5 | opencode-review.md | Major | **Agree — partial** | 140 does not span a matrix leaf. Live width **56** → RowsPerBlock **145**, not 154. DoD-3 + >RowsPerBlock fixture |
| opencode-6 | opencode-review.md | Minor | **Agree — fold** | Document `add_subnode_leaf` ~340; fail closed; §6 / §10.4 |
| opencode-7 | opencode-review.md | Major | **Already covered** | Dedicated row-matrix tree already locked |
| opencode-8 | opencode-review.md | Minor | **Already covered** | Re-verify RowsPerBlock at execute |
| opencode-9 | opencode-review.md | Minor | **Already covered** | Keep injected QC test |
| agy-0100-1 | agy-review.md | Major | **Agree — fold** | Same as opencode-1 (agy named only `load_tc`; we take the stronger four-site fix) |
| agy-0100-2 | agy-review.md | Major | **Already covered** | RowsPerBlock; decline “row 158” (depends on width) |
| agy-0100-3 | agy-review.md | Major | **Agree — fold** | HID `((block as u32) << 16) \| ((hid_index as u32) << 5)`; §10.1 |
| agy-0100-4 | agy-review.md | Major | **Already covered** | HNBITMAPHDR fail closed |

**Declined / not locked**

- Expanding 0100 to implement HNBITMAPHDR or attach-table Strategy A.
- Changing BCC default.
- Opencode's 154-row / 12×4+3×1 width arithmetic (wrong vs live 15-col schema).

---

## 3. In scope

1. Recipient TC Strategy A in `pst-writer` production path (streaming unique-pst included).
2. Row-matrix subnode + RowsPerBlock data tree; `hnidRows` NID; table node `bid_sub` non-zero when rows exist.
3. Multi-block HN for the recipient-table heap (HNHDR page 0 + HNPAGEHDR continuations; HID `hidBlockIndex`).
4. `pst-reader` `TableContext::load` (shared by `load_tc`, `list_recipients`, embedded attach + recipient tables): `hnidRows` NID, RowsPerBlock, cell HNID (HID vs NID).
5. Invert 0093 tests that require truncation; keep the 140-row include-bcc fixture as the **all-rows + multi-page HN** case; add a separate fixture with **row count > RowsPerBlock** (execute **146** at row_width 56) for matrix spanning. Dead space is proven separately (width that does not divide 8176).
6. Docs: `docs/unique-pst-export.md`, `docs/pst-writer-fidelity-v1.md`.
7. Close `D-0093-recipient-tc-multipage` on DoD.

## 4. Out of scope (do NOT do here)

- CRC/poly **0099**. Nested depth **0101**. Oracle attest **0102**.
- Changing BCC default (**0082**). `--include-bcc-recipients` remains opt-in.
- Attachment-table TC (`D-0093-attachment-tc-page`). Shared helpers OK only if attach behavior is **byte-identical**.
- Restoring 3580 per-value semantics on **message PC** HeapBuilder.
- Implementing HNBITMAPHDR (fail closed; `D-0100-hn-bitmap-hdr`).
- Matter `extract-pst` Display* → `item_participants` (`D-0018-03` residual).
- Client PSTs in git; COM Outlook automation. Optional operator Outlook **open** of a synthetic unique-pst is evidence, not CI.
- In-tool repair / ScanPST of source stores.

## 5. Preconditions & dependencies

- **P1 (blocking):** 0093 Strategy B remains on `main` until A is proven in tests. Do not delete the truncate QC branch until A is wired (then stop **emitting** events).
- **P2:** `pst-reader` Heap already understands `hidBlockIndex` (verified).
- *Verified to date:* INC* 39/3082 after 0098; live B path and TCINFO `hnidRows` patch at `production.rs` ~4669–4672.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Naive `write_data_chain` splits a row | Dedicated row-matrix writer; unit test that a matrix just over one block does not split `row_width` |
| `load_tc` concat of all subnodes | Shared `TableContext::load`; test with matrix + one extra string subnode; cover `list_recipients` / embedded recip too |
| 140-row fixture never spans a matrix leaf | Add **>RowsPerBlock** row-count fixture (145 at plan-time width 56) |
| `add_subnode_leaf` ~340 entry ceiling | Fail closed (already typed); ordinary tables are matrix NID + rare >2048 cell NIDs |
| Outlook rejects multi-block HN / packing | Follow §2.3.4.4 block size + dead space; optional operator Outlook open (HITL) |
| HID `hidIndex` overflow into `hidBlockIndex` | Per-page index; fail closed at 2047 allocs on a page (start next page on byte budget first) |
| Bitmap page needed | Fail closed; residual `D-0100-hn-bitmap-hdr` |
| Strategy B tests left asserting truncate | Invert in the same PR; CI must not keep “truncate required” |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Rows:** Synthetic fixture with **≥136** included recipients (the 0093 **140-row** include-bcc case remains canonical for all-rows + **multi-page HN**): unique-pst / `write_unicode_pst` writes **all** included TC rows. `recipient_tc_truncated_messages == 0`, `recipient_rows_truncated == 0`, no `RECIPIENT_TC_TRUNCATED` events. Reader (and QC if run) sees the full To/Cc/(Bcc-if-flag) set. Display* still full. **140 rows do not span a row-matrix leaf** (width 56 → RowsPerBlock **146**); matrix spanning is DoD-3.
- [ ] **DoD-2 — BCC:** Default omit still drops BCC rows; `--include-bcc-recipients` writes them. No new BCC policy.
- [ ] **DoD-3 — Layout:** Non-empty recipient TC has `hnidRows` NID; row matrix packed with integral rows per 8176-payload block; recipient-table `bid_sub` non-zero. Empty tables: **`hnidRows = 0`**, `hidRowIndex = 0`, `bid_sub = 0` (today's unconditional `try_alloc(&row_matrix)` must stop). At least one fixture with **row count > RowsPerBlock** (execute **146**; `8176 / row_width`) proving no mid-row split and exact reader count.
- [ ] **DoD-4 — Reader:** `TableContext::load` (not `load_tc` only) does not concatenate sibling SLENTRYs; RowsPerBlock dead space ignored; `get_row_string` (and any other cell-HNID readers) resolve HID → heap / NID → table subnode. QC To/Cc mismatch without truncate event remains **defect**. The four call sites (`load_tc`, `list_recipients`, embedded attach table, embedded recipient table) must not pre-concat.
- [ ] **DoD-5 — Fail closed:** Heap/HID/bitmap/SLBLOCK exhaustion returns `WriterError` (typed), not a cap. Existing 0093 “truncate required” tests are gone or inverted. Keep `recipient_tc_truncate_event_is_known_gap_not_defect` (injected event).
- [ ] **DoD-6 — Docs + deferred:** `docs/unique-pst-export.md` and `docs/pst-writer-fidelity-v1.md` describe Strategy A; `D-0093-recipient-tc-multipage` closed; residuals listed in §9 exist on disk.
- [ ] **DoD-7 — Recorded:** `review.md`; registry **Completed**; ledger commit (category `FEATURE` or `BUGFIX`). Optional INC* re-smoke (`output/inc0102784-post-0100/`) is HITL, not CI.

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
$env:CARGO_TARGET_DIR = 'C:\dev\Dedupe\target'
cargo test -p pst-writer
cargo test -p pst-reader
cargo test -p pst-dedup-cli recipient
cargo fmt --all --check
cargo clippy -p pst-writer -p pst-reader -p pst-dedup-cli --all-targets -- -D warnings
# before publish:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

## 9. Deferred absorb / decline

| ID | Disposition |
|---|---|
| `D-0093-recipient-tc-multipage` | **Absorb — close** on DoD |
| `D-0094-inc-resmoke` | **Partial** — optional operator INC* after 0100; still lists 0101 depth |
| `D-0093-attachment-tc-page` | **Decline** — not this track |
| `D-0018-03` matter extract participants | **Decline** — reader half closed 0082; matter path is not unique-pst |
| `D-0080-bcc-policy` | **Already decided** — do not reopen |
| `D-0068-01` / 3580 message-PC | **Decline** — 0093 closed diversion; 3580 restore stays residual research |
| `D-0099-oracle-inputs-attest` | **Mint 0102** — not folded here |
| `D-0097-window-edge-normalize` | **Defer** P3 polish — not Series P |
| `D-0100-hn-bitmap-hdr` | **Spawn** if fail-closed bitmap path is hit or left unimplemented |
| `D-0062-codesign` | **Decline** — release ops |
| `D-0077-poly-fingerprint` / `D-0099-attach-crc-job-level` | **Decline** — 0099 residuals |

---

## 10. Design (locked)

### 10.1 Write path

Replace `build_recipient_table_budget_aware` with a single build that:

1. Filters BCC (0082), then To→Cc→Bcc **order** (stable; still useful for Outlook, no longer a cap order).
2. Builds row bytes as today (column schema / CEB unchanged).
3. Allocates per-row variable values on a **multi-page-capable** heap (`try_alloc` starts a new HNPAGEHDR page when the current page would exceed `MAX_BLOCK_DATA` or 2047 allocs). HID encoding: `((hid_block_index as u32) << 16) | ((hid_index as u32) << 5)` with `hid_index` **1-based per page**. Values &gt; `MAX_HEAP_VALUE_SIZE` → subnode NID in the cell (**new** in this builder — not present on `main` today).
4. Writes the row matrix as a **subnode data tree** packed with `RowsPerBlock = 8176 / row_width` (integer division). Each non-last leaf payload is 8176 bytes (physical block 8192 with trailer). Last leaf may be shorter. Dead space is padding, not rows.
5. Sets TCINFO `hidRowIndex` = BTH HID (may live on page 0 or later); `hnidRows` = matrix subnode NID; `hidIndex` deprecated field **0** (MS-PST MUST).
6. Finalizes table node: `bid_data` = HN bytes (possibly XBLOCK if multi-page HN is stored as a data tree of HN pages — follow §2.3.1.6: HN spanning multiple blocks). `bid_sub` = SLBLOCK listing matrix NID (+ any cell-string NIDs).
7. On any layout failure: `Err(WriterError::…)`, **do not** shrink `keep`.

Empty table: `hnidRows = 0` (do **not** `try_alloc` an empty matrix); `hidRowIndex = 0`; `bid_sub = 0`.

### 10.2 Read path

`TableContext::load` owns resolution (all four call sites stop pre-concatenating):

1. Parse HN from table `bid_data` (`block_size = 8176` when multi-block).
2. Parse TCINFO from `hidUserRoot`.
3. If `hnidRows == 0`: zero rows.
4. Else if HID: `heap.get` (legacy inline).
5. Else NID: resolver `find_subnode_entry` for **that** NID only; assemble data tree; **RowsPerBlock** per leaf; ignore dead space; concatenate **logical rows only**.
6. `get_row_string` (and any other variable-cell readers): HID → heap; NID → that subnode's bytes. There is no `get_row_binary` today — do not invent one unless a DoD path needs it.

`load_tc` / `list_recipients` / embedded attach + recip loaders pass `bid_sub` + crypt/bbt reader into that API instead of a prebuilt `Vec`.

### 10.3 Tests (minimum)

| Test | Assert |
|---|---|
| 140-row include-bcc (replace truncate test) | 140 TC rows; 0 truncate counters; Display* full; multi-page HN exercised |
| **>RowsPerBlock rows** (160 To/Cc at width 56 → 146+14) | Exact reader count; continuation-leaf display names round-trip |
| Empty table | `hnidRows == 0`, `hidRowIndex == 0`, `bid_sub == 0` |
| Row-matrix tree (width that does not divide 8176) | Non-last leaf padded to 8176; rows do not span; dead space ignored |
| Default BCC omit on mixed To/Cc/Bcc | BCC rows absent; To+Cc complete; display_bcc known_gap only |
| Extra string subnode sibling | Reader row count unchanged (proves no SLENTRY concat) — via `list_recipients` or `load_tc` |
| Per-row string &gt; 2048 | Writer cell NID; reader `get_row_string` round-trips |
| QC | 140-row write: no `recipient_table` defect; **keep** `recipient_tc_truncate_event_is_known_gap_not_defect` |

### 10.4 Fail-closed reasons (typed `WriterError::Layout` or existing variant)

- `row_width == 0`
- `hidIndex` would exceed 2047 on a page after attempting a new page
- Next page would be an HNBITMAPHDR index (8, 136, 264, …)
- `add_subnode_leaf` payload would exceed `MAX_BLOCK_DATA` (~340 SLENTRYs)
- Subnode NID / data-tree write failure

Do not map these to `RECIPIENT_TC_TRUNCATED`.
