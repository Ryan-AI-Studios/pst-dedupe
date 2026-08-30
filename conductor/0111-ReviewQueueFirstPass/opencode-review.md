# Track review: 0111-ReviewQueueFirstPass

**Harness:** OpenCode (`opencode`)
**Track:** `conductor/0111-ReviewQueueFirstPass`
**Date:** 2026-08-29
**Mode:** review only — no implement, no fold.

## Summary

Every load-bearing §2.2 API pin was verified against live code at HEAD `0058019` and holds:
`SCHEMA_VERSION == 39`, `ReviewListRow`'s exact 18 fields, `list_review_thin`'s
`(review_order IS NULL), review_order, imported_at, path, id` order (spec's "NULLS LAST" is
the right reading), all four filter entry points, the whole SavedSearch API, `apply_codes`'
empty-args / single-group-conflict rejections, `seed_default_codes`' exact group map, the
400-chunk `list_item_codes`, the Desk `22px/500/50k` constants, Tauri/Leptos pins (and
`leptos_router 0.8.15` — exact), `fts_unavailable`'s two live sources (`IndexMissing`,
`LangPackStale`), `preset_uncoded` lowering to `code_missing eq true`, `upsert_saved_search`'s
empty-name rejection, `is_encrypted_matter`, `ItemInput`'s review fields, `include_family`,
`DEFAULT_FTS_FETCH_LIMIT 50_000`, `deny.toml`'s already-present `matter-search` exception,
and the CI `chrome-ui` job (wasm + trunk 0.21.14 + host tests). The mock's forbidden surface
(REDACT/WITHHOLD/CANDIDATE pills, fake "2,118" QC counts, `/` `=` `/review` queue) reproduces
exactly as §2.3 says. The honesty design — `code_missing=true` as Not-Exists SQL (so
"Unreviewed" means *no code at all*, including hot/confidential), privilege-as-coding,
Control#-not-Bates, family-size ≠ attachment-count, `total` ≠ DOM count — is the same
honesty spine that made 0110 land.

The one real hole: **the chrome host cannot compute §3.3's batched `family_size` honestly
today** without either violating the "don't extend matter-core" lock or issuing one query
per family. Everything else is fixable wording. No B.

## Findings (B/M/m/O)

No B. Five m.

### m1 — `family_size` batch has no honest implementation path

§3.3: "family size: count of items sharing `family_id` (batch). `family_id` null → **1**."
But: `ReviewListRow` carries no custodian/codes/family-size; `list_family_members`
(`matter.rs:2637`) is per-family and errors if the family doesn't exist; the only batched
chunked helper is `children_map` for **parents** (`:4044`, chunk 500) — it takes
`parent_item_id`s, not `family_id`s; `Matter::connection()` (`:1613`) is pub "for advanced
callers", but hand-writing `SELECT family_id, COUNT(*)` SQL in the chrome host inverts the
crate boundary (`matter-core` owns SQL) and silently forks the family-size definition. And §4
explicitly freezes "Extending `ReviewListRow` in matter-core". So the honest batch needs a
small matter-core addition (e.g. `family_sizes(&[family_id])` beside `children_map`), which
the spec neither permits nor forbids — an implementer hits this on day one.

Fix: amend the §4 lock to "Extending `ReviewListRow` is out; a minimal batched
**family-size count** helper in `matter-core` (mirroring `children_map`'s chunk 500) is
allowed," or pin the per-page fallback (≤500 ids, one SQL per unique non-null family_id,
still windowed-but-not-batched) and accept it as a known cost. Either is one sentence; right
now it's a lock contradiction.

### m2 — Extras shape contradicts itself: `extras: bool` vs a response with no extras

§3.3 says the same command "may include optional `extras: bool`, default false", and the
`QueueRow` block says `withhold` is "false when extras=false" — but the pinned response
`{ total, offset, limit, rows: QueueRow[] }` has no `extras` field in `review_queue_page`
and (by §3.8) `review_codes_preview` only covers code membership; it never returns
custodian/withhold, so nothing in the spec's command set can carry lead/QC extras at all.
The plan (Phase 1) doesn't schedule it either — the lead/QC toggle has **no data source**
in the plan's own task list. Concretely: either (a) pin the toggle's data as a second invoke
(`review_queue_page` with `extras: true`, response grows an `extras: { custodian, withhold }`
map keyed by row id, or a dedicated command), or (b) defer the toggle to 0112 explicitly.
Right now DoD-1 ("toggle exists and is off") is testable but **unimplementable** per the
assembled spec.

### m3 — `.queue-row` DoD selector: pinned CSS class never appears in the UI spec

DoD-3 gates DOM windowing on "count of `.queue-row` (or `[data-queue-row]`) ≤ 64". §3.5 pins
32px rows + overscan, 2+16+2+16 = **36** rows per window; at rest (scroll_top=0, viewport
640px) that's ≤ 36, comfortably ≤ 64 — the math is fine. But the UI section (Phase 2, §3.9)
never names the class/attr. If the implementing dev styles rows as `class="queue-item"` /
plain `<tr>`, DoD-3's querySelector fails and the check drifts to "document in review.md as
HITL-only". One line in §3.9 fixes it: pin `class="queue-row"` (Hermes/mock precedent) on
every rendered queue row, and assert on that exact selector.

### m4 — Bulk-tag N is selection-first, preview can be smaller — say which one the confirm shows

§3.8 gates the privilege confirm on "preview says N items would gain or lose that
membership" and DoD-4 asserts `privilege_would_change=3` for 3 selected. Live `apply_codes`
(`matter.rs:4745-4763`) rejects *conflicting* single-group adds in one batch, but selecting 3
rows where **2 already carry `privilege`** yields preview N=2, not 3 (re-applying an existing
membership neither gains nor loses). That's correct honesty — but the spec should pin that
the confirm shows the **preview's** N ("This changes Privilege coding on 2 items"), not the
selection size, and that already-coded rows yield `N` smaller than `selected`. Also note the
preview helper must be built from `list_item_codes` + catalog diffing in chrome (matter-core
has no preview API); worth one sentence so it doesn't get invented as a matter-core change.

### m5 — `insert_item` for the 1000-row test: pin how `in_review` gets set / ids come back

`ItemInput` carries `in_review`/`review_set_id`/`review_order` as `Option` fields
(`matter.rs:444-446`) — nothing auto-defaults them. DoD-2 needs 1000 items in the default
review set with deterministic ids for the "offset=50 disjoint" assertion. Plan Phase 1 says
"1000-row page/total/disjoint offset" but never pins: (a) `ensure_default_review_set("…")`
returns a `ReviewSet` — use its id for every `ItemInput.review_set_id`, (b) set
`in_review = Some(1)` per row (else `scope=review_corpus` count is 0 and the whole test
vacuously passes), (c) generate ids (`format!("it_{i:04}")` or `insert_item`'s returned
`Item.id`) so offsets are comparable. One sentence in Phase 1; without it DoD-2 is a coin
flip on whatever defaults the implementer guesses.

## What looks solid

- **`total` honesty is load-bearing and correctly derived**: `compose_keyword_filter`
  (`matter-search/src/compose.rs:14-38`) returns count from `count_items_filtered[_in_ids]`
  on the same filter+hits the page rows come from — `total` ≠ `rows.len()` falls out of the
  API shape, not developer discipline. The empty-hits early return (`:48-49`) makes
  "keyword matched nothing" honest `total=0`, while missing index errors before any count —
  so `fts_unavailable` vs `total=0` can't be conflated.
- **`fts_unavailable` is grounded in two live error variants** (`SearchError::IndexMissing` →
  `"fts_index_missing"`, `LangPackStale` → both in `error.rs:51-52`), and `run_fts_index`
  is the Desk-side escape hatch the message should name (spec says "run fts_index in
  Desk/Process" — matches `run.rs:75` + Desk workspace menu).
- **Unreviewed semantics are *stricter* than the mock's**: `preset_uncoded` + live SQL
  (`filter.rs:657-668`) means "no codes whatsoever" — Not-Exists on `item_codes`. A row with
  only `hot` coded is **not** Unreviewed. That matches Desk 0028 and is the honest default;
  §3.3's "that sends `code_missing`" is exactly right.
- **Privilege pill honesty**: `preset_privilege` filters `cd.key = 'privilege'` (`:115-125`),
  and `ItemPrivilege.withhold` lives in privilege.rs as a 0/1 **production hold** (`:111-118`
  "0/1 production hold") — the spec's coding-vs-treatment split mirrors the schema's own
  split. Confirmed: `list_item_privilege` (`privilege.rs:699`) is a separate API; nothing in
  the queue path leaks withhold into first-pass columns.
- **Caps chosen against the right evidence**: Desk constants verify exactly (`ROW_HEIGHT`
  22px, `THIN_PAGE_SIZE` 500, `THIN_LOAD_ALL_THRESHOLD` 50k, `show_rows` viewport-scoped code
  loads at `review_ui.rs:83-93,523,1326`), and the spec explicitly *refuses* the 50k
  load-all on wasm. `list_items_filtered_thin_in_ids` uses a **temp-table hit intersection**
  (`with_fts_hit_temp`, `matter.rs:3964-3979`), not a 50k-IN-list — so even the keyword path
  can't blow SQL var limits. Chunk 400 in `list_item_codes` (`:4663`) matches the spec's
  "chunk 400" note verbatim.
- **Route/host state is exactly as §2.2 says**: lib.rs registers precisely
  `matter_overview`, `create_matter`, `recent_matters_list`, `recent_matters_remember`
  (`lib.rs:66-71`); capabilities `default.json` lists those four `allow-*` + `core:default`
  + `dialog:default`, no `fs:*`; `ReviewStub` still on `/matters/:id/review`
  (`ui/src/app.rs:136`); CSP carries `'wasm-unsafe-eval'` + `ipc:` (0110's M1 fix landed).
  Spec's "Capabilities list those four only" is exact.
- **`encode_matter_id`/`decode_matter_id` already exist** (`path_id.rs:35-40`) from 0110 —
  the 0110 review's o1 (round-trip encoding test) is already absorbed (test
  `literal_percent_in_root_not_double_decoded_from_params` at `:97`). The 0111 `:docId` route
  reuses the same helper — spec's parenthetical "(percent-encoded if needed)" is fine.
- **`CommandError` kinds are exactly the three §3.3 reuses** (`not_found`/`encrypted`/
  `failed`, `error.rs:12-31`) — `fts_unavailable` is a new kind this track, correctly
  specified as such.
- **Workspace plumbing is already done**: `matter-search` is a workspace member
  (`Cargo.toml:21`), `deny.toml:90` already exempts it, host already depends on
  `matter-core:21`. §3.1's conditional "add exception only if deny fails" is a no-op today —
  correctly framed as conditional rather than asserted.
- **Mock reproducibility**: queue.rs really has the REDACT/WITHHOLD/CANDIDATE pills
  (`:48-50`), fake QC counts ("2,118" / "96", `:292-293`), and `/` `/review` both mounting
  the queue (`app.rs:12-13`) — every §2.3 "do not copy" item is checkable and checked.

## Deferred fold-in table

| Row | Live state (`docs/deferred.md`) | Spec disposition | Verdict |
|---|---|---|---|
| **D-0111-first-pass-queue** | :920 — open, "Absorb on Implement (0111 Ready)" | Absorb / close | ✅ |
| D-0026-01 | :116 + newer row :142 (improved in 0028, residual) | **Partial** — chrome window + 500 cap; Desk residual stays | ✅ honest partial |
| D-0026-03 | HTML/image body → 0112 | Decline | ✅ |
| D-0028-01 keyset / D-0028-02 nested OR | :138 / :139 | Decline (OFFSET; flat AND) | ✅ |
| D-0027-03 auto-propagate | :129 / :587 | Decline; propagate off | ✅ matches "never default" |
| D-0112 / D-0113 / D-0116 | Proposed | Route stubs only | ✅ |
| D-0032-01 / D-0034-02 → 0114; D-0040-01 / D-0060-04 → 0115 parked | open | Decline | ✅ |
| D-0110-deny-unic | :919 residual/upstream | Remain | ✅ |
| D-0062-codesign | open | Decline (HITL is unsigned release EXE) | ✅ |
| D-0108 / D-0067 | unique-export / matter children | Decline | ✅ |
| D-0020-01 | egui smoke | Analog HITL owner-local | ✅ |
| `W/n` proximity, Local-AI first-pass | not minted | Decline / not minted | ✅ no 0117 |

No open med/high row overlaps the queue surface. Next free ID **0117** stands.

## Cursor / last-PR comments the plan missed

PRs **#112, #111, #110, #109** all merged; `gh pr view` on #111 and #112 → **0 comments, 0
review bodies**. §2.8's "none" dispositions are correct. No new placeholder; 0117 stands.

## Research / tools notes

- **ai-brains: used** from `C:\dev\Dedupe` — preflight (inited; **3890** pinned; spec §2.5
  says 3888, now 3890 — self-correcting drift, noted); `sync query` recovered decision
  `6a773081` **verbatim-matching the spec's product locks**: Unreviewed default via
  `preset_uncoded`, lead/QC toggle not default, privilege = `item_codes` PRIV not
  REDACT/WITHHOLD, Control# = `review_order`, page limit max 500, 32px/overscan 8, no
  `use_infinite_scroll` corpus, `fts_unavailable` not total=0, Enter → 0112 stub.
- **ledgerful: used** from `C:\dev\Dedupe` — doctor readyForPublish **true** (standing
  warns: phantom-promote legacy, sig-pin, sig-version, stale hook template, 8081/8083
  optional-model unreachable; none block); ledger status **0 pending / 0 unaudited drift**;
  planning tx `243ca2fa` found in ledger search (`conductor/0111-ReviewQueueFirstPass`,
  Docs, 22:33); `scan --impact` **LOW** (dirty tree = conductor registry + deferred lines +
  `.claude` junction + root `agy-review.md`, no product crates).
- **Online research: applied** — crates.io live: `tauri` max stable **2.11.5** (no 3.x),
  `leptos` **0.8.20**, `leptos_router` **0.8.15** (exact §2.4 pin), `leptos-use` **0.19.0**.
  Leptos-use docs: `use_infinite_scroll` exists, **`use_virtual_list` does not** (404 +
  docs.rs grep) — §2.4's claim verified current. MS-PST: N/A per spec, confirmed nothing in
  the queue touches PST parsing. Trunk 0.21.14 pin re-confirmed in live CI
  (`ci.yml:106 --version 0.21.14`).

## Verdict: Ready after fixes

No B findings. Fold in before implement start (all small):

1. **m1** — resolve the family-size lock: name the allowed minimal `matter-core` batched
   count helper (mirroring `children_map`'s shape), or pin the ≤500 per-page per-family
   fallback. DoD-2's family assertion currently has no legal implementation path.
2. **m2** — pick the extras shape (second invoke vs grown response vs dedicated command) so
   the lead/QC toggle has a data source; or defer the toggle to 0112 in DoD-1.
3. **m3** — pin `class="queue-row"` (name the exact DoD selector) in §3.9.
4. **m4** — confirm dialog shows preview-derived N (may be < selected); note
   `privilege_would_change` comes from chrome-side `list_item_codes` diffing, not a
   matter-core API.
5. **m5** — Phase 1: pin `ensure_default_review_set` id + `in_review=Some(1)` + deterministic
   ids for the 1000-row fixture.

`/foldin 0111` folds this file into spec/plan (fold review files only; do not implement here).