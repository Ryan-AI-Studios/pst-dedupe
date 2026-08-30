# 0111 — First-pass review queue

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export (0108–0109),
> matter-home overview (**0110**), three-pane coding (**0112**), produce
> (**0113**), zpdf (**0114**), OPT (**0115** parked), or Process fold (**0116**).
> Do not vendor `C:\dev\dedupe-frontend`.

- **Track ID:** 0111-ReviewQueueFirstPass
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes `E-Discovery — ideal frontend` + `E-Discovery — recommended stack`. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-29); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (density, not the 13-col lead/QC default, not coral).
- **Status:** Completed (PR **#113** / `3c4ca65`)
- **Depends on:** **0110 Completed** (PR **#111** / `5a76f0b`) · Desk review list **0026–0029** (`list_review_thin` / `FilterSpec` / `compose_keyword_filter`) · coding **0027** (`apply_codes` / `seed_default_codes`) · privilege **0031** (`list_item_privilege`) · `matter-core` schema **v39**
- **Spec authored:** 2026-08-29 (placeholder → Ready)
- **Series:** O (Review chrome) — second track
>
> **Closes / absorbs:** `D-0111-first-pass-queue` (this track). Partial chrome absorb of **D-0026-01** (windowed list) — Desk egui residual stays. Does **not** close D-0028-01/02, D-0032-01, D-0112, D-0113.
> **HITL:** owner launches the **release** EXE, opens a **synthetic** 1k-row matter, confirms first-pass columns + footer count + that `.queue-row` DOM count stays windowed. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-08-29):** PRs **#112, #111, #110, #109** — no Cursor/Bugbot/review comments. Disposition in §2.8.
>
> **Review fold-in (2026-08-29):** `opencode-review.md` + `agy-review.md`. Disposition in §2.10 and `foldin-note.md`. Locks: `Matter::family_sizes` (chunk 500; do **not** extend `ReviewListRow` or raw-SQL in chrome); `review_queue_page.extras` bool; `class="queue-row"`; privilege confirm uses preview N (may be &lt; selected); 1k fixture `in_review=Some(1)` + deterministic ids.
>
> **Stack lock (inherit 0110):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / blocker only. No daemon. No process-runner. Search builder **folds into this track** (no 0117).

---

## 1. Objective

Replace the **0110** Review stub with a **virtualized first-pass queue** on the same `dedupe-chrome` EXE: thin rows from `matter-core` / `matter-search`, saved-search chips as the queue, keyboard to a **0112 stub**, bulk tag with a **privilege-change preview**. Lead/QC is a **toggle**, not the default.

This advances **product correctness** by putting counsel-facing review on the **same** review corpus, `FilterSpec`, and `item_codes` Desk already uses — not 60k DOM rows, not mock REDACT/WITHHOLD-as-coding, not a second index.

## 2. Context (read before starting)

### 2.1 Why this track, now

**0110 Completed** (PR **#111** / `5a76f0b`): matter list/home + one `matter_overview`. Continue review still lands on “First-pass queue is 0111.” Unique-export Series S is closed. The remaining product gap after chrome is the **400-docs/hour loop entry**: a windowed queue over live `in_review` rows.

### 2.2 Live APIs (plan-time 2026-08-29, HEAD `0058019`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 39` |
| `Matter::list_review_thin(set, limit, offset)` | Thin `ReviewListRow`; no body, no codes, **no custodian**. Order: `review_order` NULLS LAST, `imported_at`, `path`, `id`. |
| `ReviewListRow` | `id`, `review_order`, `role`, `parent_item_id`, `subject`, `from_addr`, `sent_at`, `received_at`, `path`, `file_category`, `mime_type`, `size_bytes`, `text_sha256`, `html_sha256`, `dedup_role`, `cull_status`, `attachment_count`, `family_id` |
| `FilterSpec` (`filter.rs`) | Flat **AND** only. `scope` `review_corpus` (default) / `entire_matter`. `include_family`. `FILTER_SPEC_VERSION == 1`. Presets: `preset_uncoded`, `preset_privilege`, `preset_responsive`, `preset_withheld`, … Bound params only. |
| `count_items_filtered` / `list_items_filtered_thin` | Filtered page. Family expand = outer membership, not hit count. |
| `count_items_filtered_in_ids` / `list_items_filtered_thin_in_ids` | FTS ∩ filter (0029). |
| `SavedSearch` / `list_saved_searches` / `get_saved_search` / `upsert_saved_search` / `delete_saved_search` | `filter_json` + optional `keyword`. Live re-run. |
| `matter-search::compose_keyword_filter` | Empty keyword → metadata only. Else Tantivy then intersect. `DEFAULT_FTS_FETCH_LIMIT` **50_000**. Missing index → `SearchError::IndexMissing` (`fts_index_missing`). |
| `list_item_codes` | Viewport/page ids; chunk 400. |
| `apply_codes(ApplyCodesInput)` | `propagate_family: bool` (default **must stay false** in UI). `actor: String`. Empty ids / empty add+remove → error. Single-group conflict rejected. |
| `seed_default_codes` | Idempotent: `responsive` / `not_responsive` / `needs_second_look` (group `responsiveness`, **single**); `privilege` (group `privilege`, **multi**); `hot`; `confidential`. |
| `list_item_privilege` | Batch claims; withhold is **treatment**, not first-pass coding. |
| `insert_item` | **pub**; writes `in_review` / `review_set_id` / `review_order` **only if the `ItemInput` sets them** (no auto-default). Tests must set `in_review = Some(1)` or `review_corpus` count is 0. |
| `list_family_members` | Per-family, full `Item` rows, errors if the family row is missing. **Not** a batch count. |
| `family_sizes` | **Does not exist today.** This track adds `Matter::family_sizes(ids: &[String]) -> HashMap<String, u64>` (chunk 500, `COUNT(*)` by `family_id` for this matter). Do **not** extend `ReviewListRow`. Do **not** `connection()`-SQL in chrome. |
| `Matter::open_for_read` / `is_encrypted_matter` | Same 0110 contract. Encrypted: never `open_*`. |
| Chrome host today | Commands: `matter_overview`, `create_matter`, `recent_matters_list`, `recent_matters_remember`. Review route is a stub. Capabilities list those four only. |
| Desk analog | `review_ui.rs`: `ROW_HEIGHT` 22, `THIN_PAGE_SIZE` **500**, `THIN_LOAD_ALL_THRESHOLD` 50_000, `ScrollArea::show_rows`, codes for **visible** rows, Load more. **Do not** copy Load-all-50k into wasm. |
| CI | `chrome-ui` job already: wasm32 + `trunk` **0.21.14** + `cargo test -p dedupe-chrome`. Keep it. |
| MS-PST | **N/A this track.** |

### 2.3 Mock (research only; re-verified 2026-08-29)

`C:\dev\dedupe-frontend\frontend\src\pages\queue.rs`: 25 static rows; 13-col lead/QC grid (QC flags, REDACT/WITHHOLD/CANDIDATE, production, redaction counts). Routes `/` = `/review` = that table. **No** `/review/:docId`. Tokens coral `#ec3013`.

**Steal:** density, checkbox+indent, family count affordance, chip row idea. **Do not copy:** lead/QC as default, REDACT/WITHHOLD as the Privilege column, fake QC danger counts, 25-row unwindowed table grown toward live data, coral, ⌘K.

Hermes first-pass columns: Control#, date, from, subject, family size, Resp, Privilege. Click row → review window. Queue default = **Unreviewed**. `?` overlay. Enter / Shift+↓ open first row. Footer claims must be wired.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2.11.5** (0110 shipped) | Keep `tauri = "2"`. Reject 3.x / pre-release. |
| `leptos` / `leptos_router` | **0.8.20** / **0.8.15** | Keep `0.8` CSR. No SSR. |
| `trunk` | **0.21.14** (CI) | Do not drop `chrome-ui`. |
| `matter-search` | workspace path | **Allowed** host dep this track (0110 forbade desk/process-runner/pst-*/matter-service only). |
| Virtual list crates | see §3.5 | **Do not** add radix-leptos experimental, `ankurah-virtual-scroll`, `leptos-struct-table`, or `leptos-use` `use_infinite_scroll` as the 60k strategy. |
| `zpdf` | — | **0114 only.** |
| Rust | **stable** | CI `dtolnay/rust-toolchain@stable`. |

`leptos-use` 0.16–0.19 tracks Leptos 0.8; **there is no `use_virtual_list`** (docs 404 at plan-time). `use_infinite_scroll` **grows** the DOM — forbidden as the corpus strategy.

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3888 pinned).
- Recall: 0026 thin list + `show_rows`; 0028 FilterSpec flat AND + saved_searches; 0029 Tantivy compose; 0031 privilege coding vs withhold; Series O search builder folds into **0111** (no 0117); Plex/paper; no BCC.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` **0 pending / 0 unaudited drift** before this tx. Doctor: phantom-promote, sig-pin, completion-unreachable — none block planning.
- Ledger tx for this planning pass: `243ca2fa-bb26-4179-a2e3-15998b14bcf2`.
- `scan --impact` after spec write (docs/conductor only expected **LOW**).

### 2.6 How this advances the north star

Not UI polish: the queue must display **the same corpus and codes** as Desk Review, with an honest footer (`total` from `count_items_filtered` / compose, not DOM child count). Mounting 60k rows or showing mock WITHHOLD as first-pass privilege **fails** the track. Unique-export surfaces are unchanged.

### 2.8 Last-PR Cursor comments (mandatory)

| PR | Surface | Disposition |
|---|---|---|
| **#112** | docs 0110 Completed registry | none |
| **#111** | 0110 Tauri chrome | none |
| **#110** | docs 0109 Completed registry | none |
| **#109** | 0109 also-eml classify | none |

No new placeholder. Next free ID remains **0117**. No BCC-default track.

### 2.9 Product locks (do not invent at execute)

See §3.

### 2.10 Review fold-in (2026-08-29)

| Id | Disposition |
|---|---|
| opencode-m1 | **Agree — fold** — allow `Matter::family_sizes` in `matter-core` (chunk 500, same IN-list pattern as `list_attachment_names_for_parents`). §4 still forbids extending `ReviewListRow` and forbids chrome host raw SQL. |
| opencode-m2 | **Agree — fold** — `review_queue_page` takes `extras: bool` (default false). `QueueRow` always has `withhold` + `custodian`; fill them only when `extras=true`. Lead/QC toggle re-invokes with `extras=true`. Do **not** defer the toggle to 0112. |
| opencode-m3 | **Agree — fold** — every rendered row has `class="queue-row"`. DoD-3 queries that selector. |
| opencode-m4 | **Agree — fold** — confirm copy uses preview `privilege_would_change` N (may be &lt; selected when some rows already carry `privilege`). Preview is chrome-side `list_item_codes` + catalog diff — **no** matter-core preview API. |
| opencode-m5 | **Agree — fold** — 1k fixture: `ensure_default_review_set` id on every `ItemInput.review_set_id`, `in_review = Some(1)`, `review_order = Some(i)`, ids `itm_{i:04}`. |
| agy-F-0111-1 | **Already covered** — §3.3 + DoD-5 require `allow-*` for the six commands. Phase 1 names `capabilities/default.json` + autogenerated permission tomls. |
| agy-F-0111-2 | **Already covered** — §3.1 allows `matter-search`. Phase 1 pins `matter-search = { path = "../matter-search" }`. `deny.toml` already excepts the crate. |
| agy-F-0111-3 | **Agree — partial** — DoD-4 already says responsive does not require the privilege confirm. Also assert `privilege_would_change == 0` for `responsive` **and** `confidential`. |
| agy-F-0111-4 | **Already covered** — DoD-2 disjoint pages. Pin `HashSet::is_disjoint`. |
| agy-F-0111-5 | **Agree — fold** — `visible_range` tests at `scroll_top=0`, near-bottom, and overscroll (clamp; `start ≤ end ≤ total`). |

---

## 3. In scope

### 3.1 Placement (crate names stay 0110)

Stay in `crates/dedupe-chrome` (host member) + `crates/dedupe-chrome/ui` (excluded). **Do not** add a third crate.

Host may add:

```
crates/dedupe-chrome/src/queue.rs          # review_queue_page + extras
crates/dedupe-chrome/src/queue_window.rs   # pure visible_range helper + tests
crates/dedupe-chrome/src/codes.rs          # preview + apply_codes wrap
crates/dedupe-chrome/src/saved.rs          # list/upsert saved searches
```

UI may add `pages/queue.rs` (replace `ReviewStub` on `/matters/:id/review`), `pages/review_doc_stub.rs` for `/matters/:id/review/:docId`, `queue_window.rs` (same formula as host).

Host **must** depend on `matter-search` (`matter-search = { path = "../matter-search" }` in `crates/dedupe-chrome/Cargo.toml` — live host only has `matter-core` today). UI still has **no** `matter-core` / `matter-search` / tantivy.

`deny.toml` already has `{ allow = ["LicenseRef-Proprietary"], crate = "matter-search" }`. Add a chrome exception only if deny fails after the new dep.

### 3.2 Routes

| Route | Screen |
|---|---|
| `/matters/:id/review` | **This track** — first-pass queue (not the 0110 stub). |
| `/matters/:id/review/:docId` | **Stub:** “Review window is 0112.” Enter / row-activate land here. **0112** replaces the stub. `docId` is the item id (percent-encoded if needed). |
| `/matters/:id/search` | **Not a separate ID.** Search builder **is** the queue filter bar. |

Keep 0110 routes. `:id` remains the percent-encoded matter root.

### 3.3 Commands (host)

Keep 0110 commands. Add these (all on a **blocking worker**, never WebView / never Tokio SQL). Same encrypted / `not_found` / `failed` kinds as 0110. **Never** return subjects/bodies beyond the thin row fields already on `ReviewListRow` (subject + from are list columns — no CAS body).

| Command | Role |
|---|---|
| `review_queue_page` | Page the queue. See below. |
| `review_code_catalog` | Active `CodeDef`s (`list_code_definitions` or equivalent pub API). Seed is **not** required on every open; if catalog empty, call `seed_default_codes` once then list. |
| `saved_searches_list` | `list_saved_searches`. |
| `saved_search_upsert` | `upsert_saved_search` (name + `filter_json` + optional keyword). |
| `review_codes_preview` | Privilege-change **preview** (no writes). |
| `review_apply_codes` | `apply_codes`; `propagate_family` default **false**. Actor `"chrome"`. |

Capabilities: add `allow-*` for each new command in `capabilities/default.json` (same pattern as 0110 `allow-matter-overview`). Rebuild so `permissions/autogenerated/*.toml` exists for the six names. **No** blanket `fs:default`. Unlisted commands are rejected at invoke time (Tauri 2).

#### `review_queue_page`

Args: `{ root, filter_json, keyword, limit, offset, extras }` with `extras: bool` default **false** (omit / JSON false).

- `filter_json` empty / omitted → `FilterSpec::review_corpus()` (full corpus). UI default chip is Unreviewed (`preset_uncoded`) — that sends `code_missing`.
- `keyword` empty → **do not** call `matter-search`; use `count_items_filtered` + `list_items_filtered_thin`.
- `keyword` non-empty → `compose_keyword_filter`. If `IndexMissing` / `LangPackStale` → error kind **`fts_unavailable`** (message says run `fts_index` in Desk/Process). **Do not** return `total=0` as if there were no hits.
- `limit` default **500**, **max 500**. `limit > 500` → `failed` (do not silently clamp to 60k).
- Encrypted: kind `encrypted`, no `open_*`.
- After the page of `ReviewListRow`, fill **for those ids only**:
  - `list_item_codes` (always — first-pass Resp / PRIV / confidential keys).
  - `family_size` via **`Matter::family_sizes`** on unique non-null `family_id`s from the page (chunk 500). `family_id` null → **1** without calling the helper. Count is items in the matter sharing that `family_id` (the family unit), **not** `attachment_count`, **not** in-review-only.
  - When `extras=true` (lead/QC toggle on): `list_item_privilege` → `withhold`; `get_item` (page ids only) → `custodian`. When `extras=false`: `withhold=false`, `custodian=None`. Redaction counts **omitted** this track (no honest cheap batch API; do not fake mock “3 danger”).

Response (shape locked — fields present even when `extras=false`):

```text
{ total, offset, limit, extras, rows: QueueRow[] }
QueueRow:
  id, review_order, date (sent_at else received_at), from_addr, subject,
  parent_item_id, role, family_id, family_size,
  resp: Option<"R"|"NR"|"NSL"|…>   // from responsiveness group keys
  privilege_coded: bool            // privilege group membership (NOT withhold)
  withhold: bool                   // true only if extras && claim withhold; else false
  custodian: Option<String>        // Some only if extras; else None
```

`total` is the compose/filter **count**, not `rows.len()`. Echo `extras` so the UI cannot mix a first-pass payload with lead/QC columns.

Lead/QC toggle **off** → invoke `extras=false`. Toggle **on** → re-invoke the same page with `extras=true`. Do **not** defer this toggle to 0112.

One `open_for_read` per invoke (same 0110 shape: `is_encrypted_matter` first).

### 3.4 First-pass vs lead/QC columns

**Default first-pass (locked):**

| Col | Source | Honesty |
|---|---|---|
| checkbox | UI selection | |
| Control# | `review_order` when set, else `—` | **Not** Bates. Do **not** invent `ACME0001`. Bates/`production_items.control_number` is produce (**0113**). Tooltip: item id. |
| Date | `sent_at` else `received_at` | |
| From | `from_addr` | |
| Subject | `subject`; indent when `parent_item_id` is Some | Optional indent; family **propagate off**. |
| Family | `family_size` (§3.3) | |
| Resp | responsiveness group: `responsive`→R, `not_responsive`→NR, `needs_second_look`→NSL, else `—` | |
| Privilege | `privilege_coded` → pill **PRIV** (`#9B2C2C`) | Coding, **not** REDACT/WITHHOLD/CANDIDATE. |

**Lead/QC toggle (off by default):** additional columns custodian, withhold pill, confidential code (from `list_item_codes` already on the page), Produced `—` + “0113”. Data source = `review_queue_page` with `extras=true` (§3.3). **No** fake QC glyph counts. **No** production Image/Native/Slipsheet enums until 0113.

Footer (always wired): `{selected} selected · {total} in queue` where `total` is `review_queue_page.total`. Empty queue: “0 in queue” + Unreviewed chip still visible.

### 3.5 Virtualization (locked)

Two layers:

1. **SQL page** — `limit ≤ 500` per invoke. Wasm **must not** fetch `total` rows when `total` is 1000+.
2. **DOM window** — fixed row height **32px** (Hermes 32px desktop min / 8pt grid). Render `visible_range(scroll_top, viewport_h, 32, fetched_len, overscan=8)` only. Spacer height = `fetched_len * 32`.

Pure helper (host tests + UI copy the same math):

```text
visible_range(scroll_top, viewport_h, row_h, total, overscan) -> (start, end)
  start = max(0, floor(scroll_top/row_h) - overscan)
  end   = min(total, ceil((scroll_top+viewport_h)/row_h) + overscan)
  end - start ≤ viewport_rows + 2*overscan
  # clamp: start ≤ end ≤ total; overscroll (scroll_top > total*row_h) still clamps
```

Host tests **must** cover `scroll_top=0`, a near-bottom position, and overscroll.

**Forbidden:** `<For/>` over 1000+ rows; `leptos-use` `use_infinite_scroll` as the corpus strategy (it appends); radix-leptos experimental VirtualList; growing a wasm `Vec` to `THIN_LOAD_ALL_THRESHOLD` (50k).

### 3.6 Saved search + filter bar

- Chips: **Unreviewed** (`preset_uncoded`, default on Continue review), **Privileged** (`preset_privilege`), **Responsive** (`preset_responsive`), then named `saved_searches` (name + `total` after apply).
- Keyword box: Tantivy query string (Boolean/phrase that `matter-search` already parses). **`W/n` proximity is out** (not in `KeywordQuery`; do not mint 0117).
- Save: name + current `filter_json` + keyword → `saved_search_upsert`. Empty name rejected (API already).
- Include-family: FilterSpec checkbox (preview membership). **Not** the same as `propagate_family` on codes.
- Focus gate: when the keyword / save-name field is focused, queue shortcuts except `Esc` do not fire. **Do not steal Ctrl+F.** `/` focuses the keyword box.

Flat AND only (0028). Nested OR builder stays **D-0028-02**.

### 3.7 Keyboard (queue only — not 0112 coding)

| Key | Action |
|---|---|
| `?` | Page overlay (these bindings). |
| `↑` `↓` | Move current row (aria-selected). |
| `Enter` or `Shift+↓` | Open `/matters/:id/review/:docId` **stub**. |
| `Space` | Toggle checkbox on current row. |
| `Esc` | Close overlay / clear bulk bar if open. |
| `/` | Focus keyword. |

`1` `2` `3` `p` `r` `[` `]` are **0112**. On the queue they no-op with overlay copy “Coding shortcuts land in the review window (0112).” Windows modifiers only (Ctrl, not ⌘). Keep 0110 `Ctrl+K` on the matters list.

### 3.8 Bulk tag + privilege preview

Bulk bar when `selected > 0`: Tag… picker from `review_code_catalog`.

Before `review_apply_codes`:

1. `review_codes_preview` with the same ids / add / remove / `propagate_family=false`.
2. If any targeted def has `group_key == "privilege"` **or** `key == "privilege"`, and preview `privilege_would_change` N &gt; 0, show a confirm: “This changes Privilege coding on N items.” **N is the preview count, not the selection size** (already-privileged rows neither gain nor lose). Cancel = no write.
3. If N = 0 (no membership change) or the tag is not privilege (`responsive`, `confidential`, …), **no** privilege confirm.
4. Withhold / produce treatment is **not** a bulk tag this track.

`review_codes_preview` is implemented in the **chrome host** from `list_item_codes` + the code catalog (diff current vs post-add/remove membership). **Do not** add a matter-core preview API. **No** writes on preview. Family propagate stays **off**.

### 3.9 Tokens / a11y / CSP

Inherit 0110 §3.4 / §3.6. No `#ec3013`. PRIV pill may use `#9B2C2C`. Skip link **“Skip to queue”** (`#queue`) in addition to existing skip links. Every rendered data row **must** have `class="queue-row"` (DoD-3 selector; optional extra `data-queue-row` is fine, not required). Rows `role="row"` inside a grid/table; current row `aria-selected`. `:focus-visible` on chips and rows.

CSP object **unchanged** (`'wasm-unsafe-eval'` + IPC `connect-src`).

### 3.10 Hygiene

- Production: no `unwrap` / `expect`. `main` still returns `Result`.
- Never mutate source PSTs. Never commit client PSTs, `output/`, `evidence/`, or matter folders with mail.
- Tests: `tempfile` + `insert_item` + `ensure_default_review_set` + `seed_default_codes`. No client PST. 1k-row fixture (locked): `let set = matter.ensure_default_review_set(...)`; for `i in 0..1000` insert with `id: Some(format!("itm_{i:04}"))`, `in_review: Some(1)`, `review_set_id: Some(set.id.clone())`, `review_order: Some(i as i64)`, `status` extracted-like. Do **not** omit `in_review` (defaults to not-in-corpus).

## 4. Out of scope (do NOT do here)

- **0112** three-pane coding, Save & Next, digits 1/2/3 applying codes, Native/Text viewer, family propagate on, privilege **type** dropdown.
- **0113** produce checklist, Bates, production column values, DAT.
- **0114** zpdf / Image tab.
- **0115** TIFF/OPT (parked).
- **0116** folding egui Process.
- Local AI first-pass job / Prediction field (v1.1, never next ID).
- Nested OR / saved-search-as-condition (**D-0028-02**).
- Keyset pagination (**D-0028-01**); OFFSET stays.
- `W/n` proximity.
- Encrypted open/passphrase.
- Axum daemon, Leptos SSR, nightly, `tauri` 3.x.
- Vendoring mock tokens / 13-col default.
- Schema bump, unique-pst flags, BCC-default.
- Legal hold, TAR, auto-privilege, StoryBuilder, clawback, LFP.
- Authenticode (`D-0062-codesign`).
- Extending `ReviewListRow` in `matter-core` (codes / withhold / custodian stay host-side fills). **Allowed:** a minimal `Matter::family_sizes` helper (chunk 500). **Forbidden:** chrome `connection()` SQL that forks the family-size definition.

## 5. Preconditions & dependencies

- **P1 (blocking):** 0110 chrome crate + `matter_overview` still present. `SCHEMA_VERSION` 39. `list_items_filtered_thin`, `compose_keyword_filter`, `apply_codes`, `list_item_codes` still pub. This track **adds** `Matter::family_sizes`. Re-verify at execute.
- **P2:** Windows WebView2; CI `chrome-ui` job stays.
- **P3:** `wasm32-unknown-unknown` + `trunk` 0.21.14.
- *Verified to date:* §2.2–2.4. Last-PR comments empty.

## 6. Risks

| Risk | Mitigation |
|---|---|
| 60k DOM / wasm Vec | Max limit 500; `visible_range` tests; forbid infinite-scroll-append. |
| Footer lies (`rows.len()` vs total) | DoD asserts `total == 1000` with `rows.len()==50`. |
| Privilege column = WITHHOLD | First-pass uses `privilege_coded` only. |
| Control# = fake Bates | `review_order` or em-dash. |
| Family size = attachment_count | `Matter::family_sizes`; null `family_id` → 1. |
| IPC invoke blocked at runtime | `allow-*` for all six commands in `capabilities/default.json`. |
| Lead/QC toggle with no data | `extras=true` re-invoke; fields always present. |
| Privilege confirm N = selection | Confirm uses `privilege_would_change` (may be smaller). |
| Keyword missing index looks like 0 hits | `fts_unavailable` kind. |
| Encrypted panic | Same 0110 detect-first. |
| `apply_codes` family expand on by accident | Command + UI default false; test. |
| `cargo test --workspace` compiles leptos | `ui/` stays excluded. |
| Two pipelines | No process-runner; queue is read + coding writes via `apply_codes` only. |
| Coral / mock port | Tokens inherit 0110; no `#ec3013`. |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Queue replaces stub:** `/matters/:id/review` renders the first-pass table (not “First-pass queue is 0111.”). Continue review from matter home lands here. Default chip **Unreviewed**. Lead/QC toggle exists and is **off**. Four 0110 tabs still work. `dedupe-desk` still builds.
- [ ] **DoD-2 — Honesty of counts + columns:** Host test, tempfile, 1000 `in_review` items seeded per §3.10 (`itm_0000`…`itm_0999`, `in_review=Some(1)`, default set id, `review_order=Some(i)`): `review_queue_page` `limit=50` `offset=0` → `total=1000` and `rows.len()==50`; `offset=50` id set is `HashSet::is_disjoint` from page 0 (not merely `len==50`). Empty matter: `total=0`. After `insert_source` only (no items): `total=0`. Encrypted: kind `encrypted`, no `open_*`. Unreviewed filter on a corpus with 1 coded + 2 uncoded → `total=2`. Privilege column is PRIV vs `—`, never REDACT/WITHHOLD. Control# is `review_order` or `—`, never `ACME0001`. Family size for a parent+2 children with shared `family_id` is **3** via `family_sizes` (not `attachment_count`). `extras=false` → every row `withhold==false` and `custodian==None`; `extras=true` fills those from privilege/item. Produced not shown as `0`.
- [ ] **DoD-3 — Virtualization:** `queue_window::visible_range` unit tests: `total=1000`, `viewport_h=640`, `row_h=32`, `overscan=8` → span `≤ 20+16`; also `scroll_top=0`, near-bottom, and overscroll clamp (`start ≤ end ≤ total`). `limit>500` rejected. UI: for the 1k fixture, at rest, count of **`.queue-row`** **≤ 64**. Document in `review.md` if HITL-only; host math tests are CI-required.
- [ ] **DoD-4 — Saved search + keyboard + bulk:** Saved search upsert+list round-trip (tempfile). Keyword on a matter **without** FTS index → `err.kind == "fts_unavailable"` (not a generic `is_err()`, not `total=0`); message names `fts_index`. `?` overlay lists queue bindings. Enter navigates to `/matters/:id/review/:docId` stub copy “0112”. Preview: 3 selected, **1 already `privilege`**, adding `privilege` → `privilege_would_change=2` (confirm copy uses 2, not 3); cancel writes nothing; apply with `propagate_family=false` membership matches. Adding `responsive` **or** `confidential` → `privilege_would_change==0` (no privilege confirm).
- [ ] **DoD-5 — Tests + CI:** `cargo test -p dedupe-chrome` covers DoD-2..4 (no client PST). Workspace fmt/clippy/test + `chrome-ui` trunk job stay green. No production `unwrap`/`expect`. CSP object unchanged. Capabilities list the new commands (no `fs:default`).
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0111-first-pass-queue` closed; ledger committed (`FEATURE`). Unblocks **0112**.

**Owner HITL (not CI):** release EXE, synthetic 1k matter, first-pass columns, footer `1000 in queue` while DOM rows stay windowed, Unreviewed chip, `?`, Enter → 0112 stub. INC* waived.

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p dedupe-chrome
cargo check -p dedupe-desk
# rustup target add wasm32-unknown-unknown
# trunk build --config crates/dedupe-chrome/ui/Trunk.toml --release
```

## 9. Deferred (absorb / decline)

| Row | Disposition |
|---|---|
| **D-0111-first-pass-queue** | **Absorb / close** on Implement. |
| **D-0026-01** large corpus paging | **Partial** — chrome window + 500 cap. Desk egui Load-more residual **stays**. Do not close the row. |
| **D-0026-03** HTML/image body | **0112** / later. Decline here. |
| **D-0028-01** keyset pagination | **Decline** this track (OFFSET like 0028 P0). No 0117. |
| **D-0028-02** nested OR | **Decline.** Residual. |
| **D-0027-03** auto-propagate | **Decline.** Propagate stays off. |
| **D-0112-review-window** | Remain Proposed. Stub route only. |
| **D-0113-produce-checklist** | Remain. Produced `—`. |
| **D-0032-01** / **D-0034-02** | Remain; owner **0114**. |
| **D-0040-01** / **D-0060-04** | Remain parked; **0115**. |
| **D-0110-deny-unic** | Remain residual / upstream. |
| **D-0116-process-fold** | Remain. |
| **D-0108-keepset-crc-retaint** | Unique-export. **Decline.** |
| **D-0067-embedded-depth** | Matter children. **Decline.** |
| **D-0062-codesign** | Release ops. **Decline.** |
| **D-0020-01** | egui smoke. Analog HITL is owner-local. |
| Local AI first-pass | **Not minted** (v1.1). |
| `W/n` proximity | **Decline** (Tantivy parser has no W/n). No 0117. |
| Mock `tokens.css` retune | `C:\dev\dedupe-frontend` only. |
| Last-PR comments #112–#109 | None. |
| opencode-m1 family_sizes | **Folded** — allowed matter-core helper; ReviewListRow freeze stays. |
| opencode-m2 extras bool | **Folded** — §3.3 + DoD-2. |
| opencode-m3 `.queue-row` | **Folded** — §3.9 + DoD-3. |
| opencode-m4 preview N | **Folded** — §3.8 + DoD-4. |
| opencode-m5 1k fixture | **Folded** — §3.10 + DoD-2. |
| agy-F-0111-1 capabilities | **Already covered** — Phase 1 explicit. |
| agy-F-0111-2 matter-search dep | **Already covered** — Phase 1 pins Cargo.toml. |
| agy-F-0111-3 non-privilege preview | **Folded partial** — DoD-4 `privilege_would_change==0` for responsive + confidential. |
| agy-F-0111-4 disjoint pages | **Already covered** — DoD-2 `is_disjoint`. |
| agy-F-0111-5 window edges | **Folded** — DoD-3 top/bottom/overscroll. |

---

## Series O index (do not reorder)

| ID | Item | After this plan |
|---|---|---|
| **0110** | Matter chrome + one overview command | **Completed** (PR **#111** / `5a76f0b`) |
| **0111** | Virtualized first-pass queue | **Ready — not started** |
| **0112** | Three-pane review window | Proposed |
| **0113** | Produce checklist; DAT only | Proposed |
| **0114** | zpdf raster + geometric redact | Proposed |
| **0115** | TIFF G4 + OPT | **Parked** |
| **0116** | Fold egui Process | Proposed |

Next free conductor ID: **0117**.
