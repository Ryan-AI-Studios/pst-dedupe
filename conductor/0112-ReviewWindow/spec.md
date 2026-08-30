# 0112 — Review window (three-pane coding)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export (0108–0109),
> matter-home overview (**0110**), first-pass queue virtualization (**0111** /
> **0117**), produce (**0113**), zpdf (**0114**), OPT (**0115** parked), or
> Process fold (**0116**). Do not vendor `C:\dev\dedupe-frontend`.

- **Track ID:** 0112-ReviewWindow
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes `E-Discovery — ideal frontend` + `E-Discovery — recommended stack`. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-30); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (density, not tokens, **no** `/review/:docId`).
- **Status:** Completed
- **Depends on:** **0111 Completed** (PR **#113** / `3c4ca65`) · **0110 Completed** (PR **#111** / `5a76f0b`) · coding **0027** (`apply_codes` / `seed_default_codes` / `list_item_codes`) · privilege **0031** (`upsert_item_privilege` / `privilege_basis`) · notes **0030** (`upsert_note`) · body **0026** (`text_sha256` / `html_sha256` CAS) · `matter-core` schema **v39**
- **Spec authored:** 2026-08-30 (placeholder → Ready)
- **Series:** O (Review chrome) — third track
>
> **Closes / absorbs:** `D-0112-review-window` (this track). Partial chrome absorb of **D-0026-03** (plain text + block-aware HTML strip in the window; Image raster stays **0114**). Does **not** close D-0027-03, D-0032-01, D-0034-02, D-0113, D-0117.
> **HITL:** owner launches the **release** EXE, opens a **synthetic** 3-doc family matter from the 0111 queue, Enter into the window, codes with `1`/`2`/`3`/`p`/`Enter`, confirms coding pane `#E8EEF2` and Image stub. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-08-30):** PRs **#114, #113, #112, #111**. Disposition in §2.8. Catalog write-lock **folded here**. Three queue Bugbot items **minted 0117**.
>
> **Review fold-in (2026-08-30):** `opencode-review.md` + `agy-review.md`. Disposition in §2.10 and `foldin-note.md`. Locks: `review_window_apply` host sequence (pre-check basis → `apply_codes` → upsert; compensate on upsert fail); do **not** change `ensure_item_privilege_conn`; `family_members_thin` SQL LIMIT; `position` always counted; body `cas_len` + prefix + `from_utf8_lossy`.
>
> **Stack lock (inherit 0110/0111):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Coding pane token `--coding-pane` `#E8EEF2`. Red = privilege / withhold / blocker only. No daemon. No process-runner. No 0117 ID reuse for this window.

---

## 1. Objective

Replace the **0111** `/matters/:id/review/:docId` stub with the **money screen**: three panes (related | viewer | code) on the same `dedupe-chrome` EXE. Counsel codes **Responsiveness ⊥ Privilege+type** against live `item_codes` / `item_privilege` / CAS text — Save & Next, ditto, family card with propagate **off** by default.

This advances **product correctness** by putting the 400-docs/hour loop on the **same** review corpus and coding APIs Desk already uses — not a third radio that mixes privilege into responsiveness, not raw HTML in the WebView, not a fake Image raster.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0111 Completed** (PR **#113** / `3c4ca65`): virtualized first-pass queue; Enter lands on “Review window is 0112.” Unique-export Series S is closed. The remaining product gap after the queue is the **document coding loop**. **0114** raster waits on this window’s Image tab existing as an honest stub.

### 2.2 Live APIs (plan-time 2026-08-30, HEAD `cb4aa31`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 39` |
| `Matter::get_item` | Full `Item` (subject, from/to/cc, `text_sha256`, `html_sha256`, `native_sha256`, `family_id`, `review_order`, `mime_type`, …). `ItemNotFound` when missing. |
| `ReviewListRow` / `list_items_filtered_thin` | Unchanged. **Do not** extend `ReviewListRow`. |
| `FilterSpec` | Flat AND. Sort: `(review_order IS NULL), review_order ASC, imported_at, path, id` (`filter.rs`). `FILTER_SPEC_VERSION == 1`. |
| `list_item_codes` | Viewport/item ids; chunk 400; includes `set_at` / `set_by`. |
| `apply_codes(ApplyCodesInput)` | `propagate_family: bool`. Empty ids / empty add+remove → error. Single-group conflict rejected. Audit full target ids. Actor resolved. |
| `seed_default_codes` | `responsive` / `not_responsive` / `needs_second_look` (group `responsiveness`, **single**); `privilege` (group `privilege`, **multi**); `hot`; `confidential`. **No AEO key.** |
| `list_family_members` | Per-family, **full** `Item` rows, **no LIMIT**; `FamilyNotFound` if the `item_families` row is missing. **Do not** call from chrome (cap would be DTO-only). |
| `family_sizes` | **Exists** (0111): chunk 500, `COUNT(*)` by `family_id`. Does **not** touch `item_families`. |
| `family_members_thin` | **Does not exist today.** This track adds it (§3.5). |
| `Matter::review_neighbors` | **Does not exist today.** This track adds it (§3.4). |
| `insert_family` | Required **before** `insert_item` with `family_id` (`:2109-2117`). Parent before children (`ParentItemNotFound`). |
| `apply_codes` privilege hook | Same txn calls `ensure_item_privilege_conn` (`matter.rs:4925-4932`). Live: no row → INSERT `attorney_client` + `withhold=1`; existing ACTIVE with `withhold!=1` → **UPDATE withhold=1** (`privilege.rs:210-264`). Doc comment says already-active rows are left unchanged; the implementation still normalizes withhold to 1. **Do not change this function this track** (Desk-shared 0027/0031). Chrome sequence in §3.3. |
| `get_bytes` / `get_bytes_capped` / `read_cas_prefix` / `cas_len` | Capped get **errors** if blob `> max`. Prefix reads first N bytes without a truncated flag. Desk display cap **2 MiB** (`BODY_DISPLAY_CAP_BYTES`). Truncation = `cas_len > cap` then prefix, **not** `get_bytes_capped`. |
| `upsert_note` / `list_notes` | `NOTE_BODY_MAX_BYTES == 64 KiB`. Blank body rejected. |
| `upsert_item_privilege` / `get_item_privilege` / `list_item_privilege` | `privilege_basis::{ATTORNEY_CLIENT, WORK_PRODUCT, ATTORNEY_CLIENT_WORK_PRODUCT, COMMON_INTEREST, OTHER}`. `privilege_status::ASSERTED`. `withhold` is treatment. |
| `clear_item_privilege` | Soft-clear; description retained. |
| Chrome host today | Queue + catalog + preview + apply (apply **forces** `propagate_family=false`). Stub route `ReviewDocStub`. Capabilities: 0110 four + 0111 six. |
| `review_code_catalog` | Live: **always** `open_matter_write` (PR #113 Bugbot). This track **fixes** read-first. |
| Desk analog | `review_ui.rs` digits 1–9 on catalog order; `[` `]` / Alt+P/N prev/next; `review_body.rs` prefers `text_sha256` then `html_sha256` with block-aware strip; privilege panel in `review_privilege.rs`. |
| `html_strip` | **Desk-only** (`crates/dedupe-desk/src/html_strip.rs`). Chrome must **copy** the helper (do **not** depend on `dedupe-desk`). Logical-hash strip is **not** the display path (`HelloWorld` concatenation). |
| CI | `chrome-ui` job: wasm32 + `trunk` **0.21.14** + `cargo test -p dedupe-chrome`. Keep it. |
| Tokens | `--coding-pane: #e8eef2` already in `ui/styles/tokens.css`. |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only; re-verified 2026-08-30)

`C:\dev\dedupe-frontend`: **still no** `/review/:docId`. Queue-only mock. Coral `#ec3013`. Do not wait on the mock. Do not vendor it.

Hermes wireframe (`E-Discovery — ideal frontend` § Review): RELATED (family) | VIEWER | CODE. Responsiveness radios **and** a separate Privileged checkbox + type. Coding pane `#E8EEF2`. Save & Next primary. Ditto secondary. Apply-to-family tertiary + preview, default **off**.

Hermes **keyboard table** binds `3` = Privileged and `p` = ditto. That **mixes axes** and contradicts the same document’s wireframe (Needs review is the third responsiveness radio; Privilege is a checkbox). **This track follows the wireframe + 0111 orthogonality**, not the mixed-axis table. Explicit decline in §3.8.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2.11.5** (workspace lock) | Keep `tauri = "2"`. Reject 3.x / pre-release. |
| `leptos` / `leptos_router` | **0.8.20** / **0.8.15** | Keep `0.8` CSR. No SSR. Leptos **0.9 beta exists** — **do not** take it. |
| `trunk` | **0.21.14** (CI) | Do not drop `chrome-ui`. |
| `zpdf` | 0.13.0 | **0114 only.** Do not add. |
| Rust | **stable** | CI `dtolnay/rust-toolchain@stable`. |

Online (2026-08-30): `tauri` 2.11.5 still latest 2.x stable on crates.io; `leptos_router` 0.8.15 latest 0.8 line; leptos 0.8.20 is the 0.8 CSR pin already in `ui/Cargo.lock`. Re-verify at execute.

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3891 pinned).
- Recall: 0027 `apply_codes` + whole-family **opt-in** + digits with focus gate; 0031 privilege coding vs withhold; 0026 thin list + off-thread body; Series O search builder already in **0111** (no extra search ID here); Plex/paper; no BCC.
- Stale recall “frontend uses 0106+/0108+” superseded by Series O **0110+** Completed through 0111.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` **0 pending / 0 unaudited drift** before this tx. Doctor: phantom-promote, sig-pin, completion-unreachable — none block planning.
- Ledger tx for this planning pass: `91a1b6b4-6b40-4160-bade-6802edba5405`.
- `scan --impact` after spec write (docs/conductor only expected **LOW**).

### 2.6 How this advances the north star

Not UI polish: the window must **write the same `item_codes` / `item_privilege` / notes** Desk Review writes, with an honest viewer (truncated CAS labeled, never silent innerHTML). Mixing Privilege into Responsiveness as a third radio would poison produce-set logic (**0113** default `responsive AND NOT withheld`). Unique-export surfaces are unchanged.

### 2.8 Last-PR Cursor comments (mandatory)

Last four merged product PRs: **#114** (docs 0111 Completed), **#113** (0111 queue), **#112** (docs 0110 Completed), **#111** (0110 chrome).

| PR | Surface | Disposition |
|---|---|---|
| **#114** | docs registry | none |
| **#113** Bugbot — **Code catalog takes exclusive write lock** (`codes.rs` `open_matter_write` even when seeded) | **Agree — fold here.** Catalog is read; write only to `seed_default_codes` when active defs are empty. Window + queue share this command. §3.3 / DoD-5. |
| **#113** Bugbot — **Header breaks queue virtualization math** | **Valid — not this track.** Queue markup. Minted **0117**. |
| **#113** Bugbot — **Empty page misreports queue as vacant** | **Valid — not this track.** Queue empty-state. Minted **0117**. |
| **#113** Bugbot — **Keyboard cursor leaves the visible window** | **Valid — not this track.** Queue `current_idx` vs `scroll_top`. Minted **0117**. |
| **#112** | docs | none |
| **#111** | 0110 chrome | none |

Next free ID after this mint: **0118**. No BCC-default track.

### 2.9 Product locks (do not invent at execute)

See §3.

### 2.10 Review fold-in (2026-08-30)

| Id | Disposition |
|---|---|
| opencode-M1 | **Agree — partial** — live `ensure_item_privilege_conn` **does** force `withhold=1` (verified). Do **not** change matter-core this track. Pin host sequence inside `review_window_apply` (§3.3): pre-check basis → `apply_codes` → `upsert_item_privilege` (UI withhold/basis) → on upsert fail, compensating remove-privilege. DoD-3 asserts **final** state of that one command. Upsert-then-apply is **forbidden** (ensure would flip withhold back to 1). |
| opencode-M2 | **Agree — fold** — add `Matter::family_members_thin(family_id, limit)` (SQL `LIMIT`, thin columns, **no** `get_family` first). Chrome must not call `list_family_members`. |
| opencode-m1 | **Agree — fold** — `position` always = count of filtered rows with sort key `<=` anchor (including dropped-out Unreviewed anchors). Drop the “or 0 if not in filter” clause. |
| opencode-m2 | **Agree — fold** — §3.3 states matter-core auto-creates `attorney_client`+`withhold=1`; host pre-check is mandatory; matter-core will not reject missing basis. |
| opencode-m3 | **Agree — fold** — Phase 1 fixture: `insert_family` → parent → children; `ensure_default_review_set` before `in_review` writes. |
| opencode-m4 | **Agree — fold** — `include_on_log` default **true**; window only sends `status=asserted`; soft-clear is `clear_item_privilege`, never `status=cleared` on upsert. |
| opencode-m5 | **Agree — fold** — truncated = `cas_len > 2 MiB` then `read_cas_prefix(2 MiB)`; decode `String::from_utf8_lossy`. |
| opencode-m6 | **Agree — fold** — copy Desk `p_tags_do_not_merge_words` / `br_and_div_insert_breaks` tests; twin comment in both files. |
| agy-F-0112-1 | **Already covered** — catalog read-first (§3.3 + DoD-5). |
| agy-F-0112-2 | **Already covered** — neighbors compare sort key, not ID-in-filter (§3.4 + DoD-2). |
| agy-F-0112-3 | **Already covered + tighten** — DoD-4 forbids `HelloWorld` and requires whitespace between Hello and World (agy false-pass). |
| agy-F-0112-4 | **Already covered** — host pre-check in M1/m2; DoD-3 no membership write. |
| agy-F-0112-5 | **Already covered** — `<pre class="doc-body">` text nodes, no `innerHTML`. |

---

## 3. In scope

### 3.1 Placement (crate names stay 0110)

Stay in `crates/dedupe-chrome` (host member) + `crates/dedupe-chrome/ui` (excluded). **Do not** add a third crate. **Do not** depend on `dedupe-desk`.

Host may add:

```
crates/dedupe-chrome/src/document.rs     # review_document + neighbors wrap
crates/dedupe-chrome/src/body.rs         # review_document_body (CAS cap + strip)
crates/dedupe-chrome/src/html_strip.rs   # copy of desk html_to_review_text
crates/dedupe-chrome/src/notes.rs        # review_upsert_note
crates/dedupe-chrome/src/privilege_cmd.rs  # review_upsert_privilege
```

Keep `codes.rs` for catalog/preview; add `review_window_apply` **next to** (not replacing) `review_apply_codes`.

matter-core (allowed helpers only): `review_neighbors`, `family_members_thin`. Do **not** edit `ensure_item_privilege_conn`.

UI: replace `pages/review_doc_stub.rs` with `pages/review_window.rs` (or equivalent). Keep `/matters/:id/review/:docId`.

### 3.2 Routes

| Route | Screen |
|---|---|
| `/matters/:id/review` | **0111** queue — do **not** restyle as the money screen. |
| `/matters/:id/review/:docId` | **This track** — three-pane window. `docId` is the item id (percent-encoded). |
| `/matters/:id/search` | Still the queue filter bar (**0111**). |

Keep 0110 routes. Four workspace tabs still work. Esc from the window → queue (same matter).

### 3.3 Commands (host)

Keep 0110 + 0111 commands. **Fix** `review_code_catalog` (read-first). Add these (all on a **blocking worker**, never WebView / never Tokio SQL). Same encrypted / `not_found` / `failed` / `fts_unavailable` kinds as 0110/0111.

| Command | Role |
|---|---|
| `review_document` | Metadata + codes + privilege + family card + notes + neighbors. **No** CAS body. |
| `review_document_body` | Capped text/html CAS → UTF-8. Separate invoke so the chrome paints the coding pane first. |
| `review_window_apply` | Combined persist (§3.3 sequence). Honors UI `propagate_family` (default false). Actor `"chrome"`. |
| `review_upsert_note` | `upsert_note`. Actor `"chrome"`. |
| `review_upsert_privilege` | Withhold/basis-only edits **without** changing codes. Not the privilege-on path. |

`review_apply_codes` (queue) **stays** force-`propagate_family=false`. Do **not** change that contract.

Capabilities: `allow-*` for each **new** command in `capabilities/default.json`. Rebuild autogenerated permission tomls. **No** `fs:default`. CSP object **unchanged**.

#### `review_code_catalog` (fix)

1. `open_matter_read` + `list_code_definitions`.
2. If any `is_active != 0`, return those (filter inactive).
3. Else `open_matter_write` + `seed_default_codes` + list.
Never take the exclusive write lock on the already-seeded path. Encrypted: kind `encrypted`, no `open_*`.

#### `review_document`

Args: `{ root, item_id, filter_json, keyword }`.

- Encrypted first. One `open_for_read`.
- Missing item → `not_found` (not a stub “0112”).
- `filter_json` empty → `FilterSpec::review_corpus()`. Keyword empty → no `matter-search`. Keyword set → `compose_keyword_filter`; `IndexMissing` / `LangPackStale` → neighbors `fts_unavailable` **but still return the document** (do not hide the item because FTS is down). Neighbor fields null + `neighbors_error: "fts_unavailable"`.
- Fill: item headers (from, to, cc, subject, sent/received, mime, path, review_order, attachment_count), `list_item_codes([id])`, `get_item_privilege`, `list_notes`, family card via **`family_sizes` + `family_members_thin`** (§3.5), neighbors via `Matter::review_neighbors` (§3.4). **Never** `list_family_members`.
- Control# = `review_order` or `—`. Bates = `—` + “0113”. Do **not** invent `ACME0002`.
- `prediction`: omit or `{ present: false }` — **no** 0051/0052 call.

#### `review_document_body`

Args: `{ root, item_id, pane: "native" | "text" }`.

- Cap **2 MiB** (`2 * 1024 * 1024`), same as Desk.
- Truncation mechanism **pinned:** `cas_len(digest)` then `read_cas_prefix(digest, 2 MiB)`. `truncated = len > 2 MiB`. Do **not** call `get_bytes_capped` (it **errors** above the cap and would fail DoD-4). Do not treat prefix-only (no `cas_len`) as enough — prefix has no truncated flag.
- Decode: `String::from_utf8_lossy` (CAS bytes are not guaranteed UTF-8).
- **text** pane: `text_sha256` if set; else honest `"No extracted text"` (`empty: true`). Never invent body from subject.
- **native** pane: headers already on `review_document`. Body: `html_sha256` if set → **block-aware strip** (copied `html_to_review_text`); else `text_sha256`; else `"No native/extracted body"`. **Never** assign CAS bytes to `innerHTML`. **Never** decode `native_sha256` binary (msg/pdf/image) into the WebView this track.
- Image pane is **UI-only stub** — this command is not called for Image.
- Response: `{ item_id, pane, text, truncated, empty, digest }`. `digest` is the CAS sha used (or null).

#### `review_window_apply`

Args: `{ root, item_ids, add_code_ids, remove_code_ids, propagate_family, privilege_basis, withhold, include_on_log, privilege_description }`.

- `propagate_family` default **false** if omitted.
- Actor `"chrome"`.
- Write lock is correct here (mutating).

**Live matter-core trap (must be in the host, not assumed away):** `apply_codes` adding the `privilege` code **always** calls `ensure_item_privilege_conn`, which INSERTs `attorney_client` + `withhold=1` when no claim exists and **rewrites `withhold=1`** on an existing ACTIVE row that is not already withhold+include_on_log. Matter-core will **not** reject missing basis. Upsert-then-apply **flips withhold back to 1**. Do **not** change `ensure_item_privilege_conn` this track.

**Locked host sequence** (one command; UI must not order two IPCs):

1. If `add_code_ids` includes the privilege code (by catalog id/key): `privilege_basis` **required** and must be in `privilege_basis::ALL`. Else return `failed` **before any write**. Assert in tests: `list_item_codes` unchanged and `get_item_privilege` none.
2. `apply_codes` (interim claim may be `attorney_client` + `withhold=1` — never inspect this as the success state).
3. Immediately `upsert_item_privilege` per target: UI `privilege_basis`, `withhold` default **false**, `include_on_log` default **true**, `status=asserted`, description optional.
4. If step 3 fails: compensating `apply_codes` **remove** privilege (soft-clear) and return `failed`. Do not leave privilege-coded + withheld from a half-finished turn-on.
5. DoD-3 asserts the **final** state after this command only.

Turning privilege **off** is remove-code + `clear_item_privilege` (not `status=cleared` on upsert; that silently zeroes withhold/include_on_log).

`review_upsert_privilege` remains for withhold/basis edits when membership is already privilege-coded (no `apply_codes`).

#### `review_upsert_privilege`

Args: `{ root, item_id, basis, withhold, description }`.

- `basis` must be one of `privilege_basis::ALL`.
- `withhold` default **false**.
- `include_on_log` default **true** (host fills `UpsertItemPrivilegeInput.include_on_log`; not omitted into a compile-fail).
- Host **always** sends `status=asserted`. Soft-clear is `clear_item_privilege` only. Never send `status=cleared` on this command.

### 3.4 `Matter::review_neighbors` (allowed matter-core helper)

```text
review_neighbors(anchor_id, &FilterSpec, fts_ids: Option<&[String]>)
  -> ReviewNeighbors { prev_id, next_id, position, total }
```

- Same `WHERE` as `list_items_filtered_thin` (plus optional id-set from compose).
- Same `ORDER BY` as FilterSpec module docs.
- Compare the **anchor’s sort key** (`review_order`, `imported_at`, `path`, `id`) even if the anchor **dropped out** of the filter (Save & Next on Unreviewed after coding Responsive).
- `prev_id` / `next_id`: one row on each side of that key, or `None`.
- `total` = filtered count (not including the dropped-out anchor unless it still matches).
- `position` = 1-based `COUNT` of filtered rows whose sort key is `<=` the **anchor’s** key. **Always** this count — including the headline Unreviewed drop-out (DoD-2). Do **not** zero `position` when the anchor left the filter. `0` only if the filtered set is empty or the anchor id is missing (`not_found` already). Footer may show this as “N of T” for the slot the coded item occupied.
- Chunk/SQL stays in matter-core. **Forbidden:** chrome `connection()` SQL.
- Do **not** fetch 60k ids into wasm.

### 3.5 Family card (RELATED pane)

Always visible.

- `family_id` None → card shows this item only; `family_size = 1`; apply-to-family checkbox **disabled**.
- Else `family_sizes([fid])` for the count + **`Matter::family_members_thin(fid, 100)`** for the list.
- `family_members_thin` **pinned:** `SELECT id, parent_item_id, subject, role FROM items WHERE family_id = ? ORDER BY imported_at, id LIMIT 101` (or equivalent). Detect `family_truncated` from the extra row; return at most 100. **No** `get_family` first (orphan `family_id` still lists items; `family_sizes` already ignores `item_families`). Do **not** call `list_family_members` (full `Item` hydration, uncapped, `FamilyNotFound` if the family row is missing). Do **not** extend `ReviewListRow`.
- If `family_size > 100`, copy “showing 100 of N”.
- Indent children (`parent_item_id`). Do **not** use `attachment_count` as family size.
- Thread inclusive/spare (**0056**) is **out**. Family only.
- Apply-to-family checkbox default **off**. When on, preview uses expanded ids (`apply_codes` expand is the whole family unit, same as 0027 — not `list_family_members` ∩ filter). Confirm copy: “Apply to N family members” where N = `family_size` (the count helper, not the capped DTO length). Privilege-change preview on the expanded set via existing `review_codes_preview` (chrome-side; pass expanded ids from `family_members_thin` with a higher limit only if N ≤ 100, else N from `family_sizes` + apply_codes propagate). Cancel = no write.

### 3.6 Coding pane (CODE)

Background `var(--coding-pane)` / `#E8EEF2`. Width holds at viewport ≥1280 (Hermes desktop). Below 1280, panes stack **vertically** (code still reachable); DoD layout is ≥1280.

**Responsiveness** (single-group radio, required to Save & Next only in the sense that 1/2/3 writes one of the three; uncoded Save & Next with no pending change is just Next):

| Radio | Code key |
|---|---|
| Responsive | `responsive` |
| Non-responsive | `not_responsive` |
| Needs review | `needs_second_look` |

**Never** a fourth radio. **Never** bind Privilege here.

**Privilege** (separate):

- Checkbox ↔ `privilege` code.
- Type dropdown: Attorney-Client / Work Product / AC+WP / Common Interest / Other. Values = `privilege_basis::*`.
- Type **required** when checkbox is on, before Save & Next / `p` persist.
- Withhold checkbox **separate**, default off. Label “Withhold from produce”. Not a bulk queue tag.

**Confidentiality:** `confidential` checkbox. No AEO (not in `seed_default_codes`).

**Log note:** textarea → `review_upsert_note`. Optional. If privilege is on and note empty, **prompt** (non-blocking copy), do not block Save.

**History:** from `list_item_codes` `set_by` / `set_at` (who coded). Notes list under it. Do not dump the full audit chain.

**Buttons:** Ditto (secondary) · Save & Next (primary). Focus default: Save & Next.

**Prediction:** if rendered, disabled text “AI off”. No provider call.

### 3.7 Viewer (VIEWER)

Tabs: **Native** | **Text** | **Image** (stub) | Produced (`—` “0113”).

- Native / Text via `review_document_body`. Paint as `<pre class="doc-body">` / text node — **no** `innerHTML`.
- Truncated: banner “Showing first 2 MiB”.
- Empty: honest empty copy, not a spinner forever.
- Image: copy “No raster yet (0114).” `r` focuses this tab and no-ops the tool.
- Hits `[` `]` inside the document are **0114** / FTS paint — this track’s `[` `]` are **document** prev/next (§3.8).
- In-doc find (`/` and Ctrl+F when this pane is focused): `window.find` or a small filter on the pre text. **Do not** steal Ctrl+F on the queue (0111 already forbids that). On this route, Ctrl+F may find in the body (Hermes). Queue is a different route.

Skip link **“Skip to document”** (`#document`) in addition to existing skip links.

### 3.8 Keyboard (window only)

Focus gate: note textarea, type `<select>`, find field — except `Esc`.

| Key | Action |
|---|---|
| `1` | Apply `responsive` (replace other responsiveness). |
| `2` | Apply `not_responsive`. |
| `3` | Apply `needs_second_look`. **Not** privilege. |
| `p` | Toggle `privilege` code (confirm type if turning on). |
| `d` | Ditto (copy **last successfully applied** code snapshot in this session onto the current item). |
| `Shift+d` | Ditto + Next. |
| `Enter` | Save & Next (persist pending radio/checkbox/note + `review_neighbors.next_id` navigate). If no next, stay and copy “End of queue”. |
| `[` / `]` | Prev / next **document** in the current filter (neighbors). |
| `r` | Image tab stub. |
| `v` then `n` / `t` / `i` | Native / Text / Image stub. |
| `f` | Focus family card. |
| `?` | Overlay of **these** bindings (not the queue table). Overlay must say `3` = Needs review. |
| `Esc` | Close overlay, else back to `/matters/:id/review`. |
| `/` | In-doc find. |

**Decline Hermes table** `3` = Privileged and `p` = ditto: that mixed Relativity binding would make Needs review unreachable from the keyboard and collapse Privilege into Responsiveness. Overlay copy states the decline in one line: “Privilege is `p`, not `3`.”

Windows modifiers only (Ctrl, not ⌘). `Shift+1` = Responsive + Next is **allowed** (optional, nice-to-have; DoD does not require it if Enter exists).

Ditto snapshot: last **successful** `review_window_apply` add/remove + privilege basis/withhold for this chrome session. First document: Ditto no-ops with copy “Nothing to ditto yet.”

### 3.9 Tokens / a11y / CSP

Inherit 0110 §3.4 / §3.6 and 0111 §3.9. No `#ec3013`. PRIV pill `#9B2C2C`. Coding pane `#E8EEF2`. Focus `#0B57D0`. Rows/controls `:focus-visible`.

CSP unchanged (`'wasm-unsafe-eval'` + IPC `connect-src`). **No** Google Fonts. **No** `unsafe-inline` scripts.

### 3.10 Hygiene

- Production: no `unwrap` / `expect`. `main` still returns `Result`.
- Never mutate source PSTs. Never commit client PSTs, `output/`, `evidence/`, or matter folders with mail.
- Tests: `tempfile` + `insert_family` **then** parent **then** children + `ensure_default_review_set` **before** any `in_review=Some(1)` + `seed_default_codes` + `put_bytes` for CAS. No client PST. Copied `html_strip.rs` carries a comment `// mirror of crates/dedupe-desk/src/html_strip.rs; update both` and the Desk unit tests `p_tags_do_not_merge_words` + `br_and_div_insert_breaks`.
- `ui/` stays workspace-excluded.

---

## 4. Out of scope (do NOT do here)

- **0117** queue header/spacer, empty-page vacant lie, arrow `scroll_top` (PR #113 queue Bugbot).
- **0113** produce checklist, Bates, Produced tab content, DAT.
- **0114** zpdf / Image raster / redact tool / in-doc hit nav.
- **0115** TIFF/OPT (parked).
- **0116** folding egui Process.
- Queue column/virtualization changes (except catalog lock shared with queue).
- Changing `review_apply_codes` to honor propagate (queue must stay false).
- Thread inclusive/spare tree, Pop out, markup QC list, persistent highlight sets.
- AEO confidentiality, AI Prediction writes, auto-privilege, TAR.
- Nested OR (**D-0028-02**), keyset pagination (**D-0028-01**), `W/n`.
- Encrypted open/passphrase.
- Axum daemon, Leptos SSR, nightly, `tauri` 3.x, leptos 0.9.
- Vendoring mock tokens.
- Schema bump, unique-pst flags, BCC-default.
- Legal hold, clawback, LFP, Authenticode.
- Raw HTML / PDF / image bytes in the WebView.
- Extending `ReviewListRow`. **Allowed:** `Matter::review_neighbors` and `Matter::family_members_thin`. **Forbidden:** chrome `connection()` SQL; chrome `list_family_members` for the card.
- Changing `ensure_item_privilege_conn` / Desk auto-withhold on privilege coding. Chrome compensates via §3.3 sequence.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0111 queue + stub route still present. `SCHEMA_VERSION` 39. `get_item`, `apply_codes`, `list_item_codes`, `family_sizes`, `upsert_note`, `upsert_item_privilege`, `read_cas_prefix` / `cas_len` still pub. This track **adds** `Matter::review_neighbors` and `Matter::family_members_thin`. Re-verify at execute.
- **P2:** Windows WebView2; CI `chrome-ui` stays.
- **P3:** `wasm32-unknown-unknown` + `trunk` 0.21.14.
- *Verified to date:* §2.2–2.4. Last-PR: catalog lock folded; three queue items → 0117.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| 3 = Privileged (Hermes table) | Spec §3.8 + overlay copy; DoD-1 radios. |
| `innerHTML` XSS from CAS HTML | Strip + text node only; host test HTML does not round-trip tags; DoD-4 `!HelloWorld`. |
| `get_bytes_capped` errors on 2 MiB+ | `cas_len` + `read_cas_prefix`; `truncated=true`. |
| Family propagate on by accident | Window default false; queue command still forces false; tests both. |
| Privilege code without type | Host pre-check before any write (matter-core will not reject). |
| Upsert-then-apply flips withhold to 1 | Forbidden order; apply-then-upsert inside one command; compensate on upsert fail. |
| Catalog write lock vs Desk Process | Read-first; DoD-5. |
| Save & Next skips wrong item | Neighbors use sort key even if Unreviewed drop-out; `position` not zeroed. |
| 60k family SQL | `family_members_thin` SQL LIMIT 101; size from `family_sizes`; no `list_family_members`. |
| Mixing queue keyboard into window | Window bindings only on this route; Esc → queue. |
| Two pipelines | No process-runner; writes via apply/note/privilege only. |
| Coral / mock port | Tokens inherit 0110; no `#ec3013`. |
| Body on UI thread | Blocking worker, same as 0111. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Window replaces stub:** `/matters/:id/review/:docId` is three panes (related | viewer | code), not “Review window is 0112.” Coding pane computed style background is `#e8eef2` / `#E8EEF2` (HITL or UI class `coding-pane` using the token). Responsiveness is three radios (`responsive` / `not_responsive` / `needs_second_look`) **and** Privilege is a **separate** checkbox. Overlay lists `3` = Needs review, `p` = privilege, `d` = ditto. Four 0110 tabs still work. Queue route still the 0111 table. `dedupe-desk` still builds.
- [ ] **DoD-2 — Persist + neighbors + family:** Host tempfile fixture order: `insert_family` → parent `itm_0000` → children `itm_0001`/`itm_0002` with that `family_id`; `ensure_default_review_set` **then** `in_review=Some(1)`, `review_order` 0..2, seeded catalog, Unreviewed filter. `review_document` on child returns `family_size==3` and the card lists parent+children via `family_members_thin` (not `list_family_members`). `review_window_apply` `propagate_family=false` on parent `responsive` does **not** code children; `true` codes all 3. After coding `itm_0000` responsive with Unreviewed filter, `review_neighbors.next_id` is `itm_0001` (anchor dropped out) and `position` is **not** 0. Empty/missing id → `not_found`. Encrypted → `encrypted`, no `open_*`. Control# is `review_order` or `—`, never `ACME0002`. Bates not shown as a fake number.
- [ ] **DoD-3 — Privilege type + withhold orthogonality:** One `review_window_apply` adding privilege **without** `privilege_basis` → `failed`; `list_item_codes` unchanged; `get_item_privilege` none (host pre-check; do not call `apply_codes`). Same command **with** `privilege_basis=attorney_client` and `withhold` omitted/false → membership + claim row; **final** `withhold==false` and basis is the one sent (not left as the ensure-hook default). Do **not** upsert-then-apply as the success path. Responsive **without** privilege → `get_item_privilege` none or uncleared. `confidential` does not require privilege confirm (reuse 0111 preview `privilege_would_change==0`).
- [ ] **DoD-4 — Viewer honesty:** `put_bytes` text `Hello review body` on `text_sha256` → text pane exact, `truncated==false`. HTML `<p>Hello</p><p>World</p>` on `html_sha256` → native pane contains `Hello` and `World` with **whitespace between** (Desk `p_tags_do_not_merge_words`: `!contains("HelloWorld")` **and** `between.chars().any(is_whitespace)` — `contains("Hello") && contains("World")` alone is a false-pass). Response text has **no** `<p` tags. Blob `2 MiB + 1` via `cas_len` + prefix → `truncated==true` and returned **char/byte** len ≤ 2 MiB. Missing digests → `empty==true`, not a fake subject-as-body. Image tab copy names **0114**.
- [ ] **DoD-5 — Catalog lock + CI:** Second `review_code_catalog` on an already-seeded matter succeeds via **read** (instrument: seed first with write, then catalog must not require exclusive lock — test by asserting catalog works after `open_for_read` handle held, or by code-path unit that calls `open_matter_read` when defs exist). `review_apply_codes` (queue) still forces propagate false when client sends true. New commands have `allow-*`. `cargo test -p dedupe-chrome` covers DoD-2..4 (no client PST). Workspace fmt/clippy/test + `chrome-ui` trunk stay green. No production `unwrap`/`expect`. CSP unchanged.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0112-review-window` closed; ledger committed (`FEATURE`). Unblocks **0113** / **0114**. **0117** stays Proposed.

**Owner HITL (not CI):** release EXE, synthetic 3-doc family from Continue review → Enter, `1` then Enter Save & Next, `p` + type, family checkbox off, Image stub, coding pane paper-blue. INC* waived.

---

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

---

## 9. Deferred (absorb / decline)

| Row | Disposition |
|---|---|
| **D-0112-review-window** | **Absorb / close** on Implement. |
| **D-0026-03** HTML/image body | **Partial** — text + stripped HTML native. Image raster **0114**. Do not close. |
| **D-0026-01** large corpus paging | Queue; **0117** / Desk residual. **Decline** here. |
| **D-0026-05** last_review_item_id | **Decline** (optional polish). |
| **D-0027-03** auto-propagate | **Decline.** Checkbox default off. |
| **D-0027-05** coding GUI smoke | Analog HITL. Residual stays. |
| **D-0030-06** markdown notes | **Decline.** Plain text. |
| **D-0030-07** notes GUI smoke | Analog HITL. Residual stays. |
| **D-0031-08** privilege GUI smoke | Analog HITL. Residual stays. |
| **D-0031-05** AI privilege prediction | **Decline.** Prediction slot empty. |
| **D-0028-01** / **D-0028-02** | **Decline.** OFFSET / flat AND. |
| **D-0032-01** / **D-0034-02** | Remain; owner **0114**. |
| **D-0113-produce-checklist** | Remain. Produced `—`. |
| **D-0117-queue-virtualization** | **Minted** this pass; remain Proposed. |
| **D-0040-01** / **D-0060-04** | Remain parked; **0115**. |
| **D-0110-deny-unic** | Remain residual / upstream. |
| **D-0116-process-fold** | Remain. |
| **D-0108-keepset-crc-retaint** | Unique-export. **Decline.** |
| **D-0067-embedded-depth** | Matter children. **Decline.** |
| **D-0062-codesign** | Release ops. **Decline.** |
| **D-0020-01** | egui smoke. Analog HITL is owner-local. |
| Hermes `3`=Privileged / `p`=ditto | **Decline** (mixed axis). §3.8. |
| AEO confidentiality | **Not minted** (not in catalog). |
| Local AI first-pass | **Not minted** (v1.1). |
| Mock `tokens.css` retune | `C:\dev\dedupe-frontend` only. |
| Last-PR #113 catalog lock | **Folded** — §3.3 + DoD-5. |
| Last-PR #113 three queue items | **Minted 0117.** |
| opencode-M1 withhold flip | **Folded partial** — host sequence, not `ensure_item_privilege_conn`. |
| opencode-M2 family cap | **Folded** — `family_members_thin`. |
| opencode-m1 position 0-clause | **Folded** — §3.4. |
| opencode-m2 auto-claim | **Folded** — §3.3. |
| opencode-m3 fixture order | **Folded** — Phase 1 / DoD-2. |
| opencode-m4 include_on_log | **Folded** — default true; asserted-only. |
| opencode-m5 truncated/UTF-8 | **Folded** — `cas_len` + prefix + lossy. |
| opencode-m6 html_strip twin | **Folded** — copy Desk tests. |
| agy-F-0112-1..5 | **Already covered** / DoD-4 tighten (F-3). |

---

## Series O index (do not reorder)

| ID | Item | After this plan |
|---|---|---|
| **0110** | Matter chrome + one overview command | **Completed** (PR **#111** / `5a76f0b`) |
| **0111** | Virtualized first-pass queue | **Completed** (PR **#113** / `3c4ca65`) |
| **0112** | Three-pane review window | **Completed** (PR **#115** / `81a3aad`) |
| **0113** | Produce checklist; DAT only | Proposed |
| **0114** | zpdf raster + geometric redact | Proposed |
| **0115** | TIFF G4 + OPT | **Parked** |
| **0116** | Fold egui Process | Proposed |
| **0117** | Queue virtualization residuals (PR #113) | **Proposed — placeholder** |

Next free conductor ID: **0118**.
