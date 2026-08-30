# Track review: 0112-ReviewWindow

**Harness:** OpenCode (`opencode`)
**Track:** `conductor/0112-ReviewWindow`
**Date:** 2026-08-30
**Mode:** review only — no implement, no fold.

## Summary

The spec's §2.2 API table verifies against live code at HEAD `cb4aa31` almost exactly:
`SCHEMA_VERSION == 39` (`schema.rs:11`); `get_item` (`matter.rs:2524`); `family_sizes` exists
(`matter.rs:4090`, chunk 500, `COUNT(*) GROUP BY family_id`, missing ids omitted — the 0111
m1 finding is properly closed); `read_cas_prefix` (`matter.rs:1713`) genuinely prefix-reads
without loading the tail (local branch `open_read` + bounded `read`, remote branch
`read_prefix_remote`); `get_bytes_capped` (`:1696`); `NOTE_BODY_MAX_BYTES == 64 KiB`
(`matter.rs:759`), blank-body rejection (`:5005`); `list_notes` newest-first (`:4980`);
`upsert_item_privilege` validates basis/status (`privilege.rs:529-534`); `privilege_basis::ALL`
is exactly the five spec values (`privilege.rs:39-44`); `privilege_status::ASSERTED` (`:50`);
`list_item_codes` chunk 400 (`matter.rs:4700`); the single-group conflict rejection
(`matter.rs:4784-4806`) and single-group clear-then-insert (`:4898-4907`) both live;
`expand_family_units` is parent + direct children + family_id members (`:5520-5568`) matching
§3.5's "whole family unit" claim; `list_family_members` errors on missing family row
(`:2637-2639` via `get_family`). Desk analog verifies: `html_to_review_text` block-aware with
`p_tags_do_not_merge_words` test (`html_strip.rs:40,172`), `BODY_DISPLAY_CAP_BYTES == 2 MiB`
(`review_body.rs:25`). Chrome-side claims verify: `review_code_catalog` really always takes
`open_matter_write` (`codes.rs:59` — the #113 Bugbot is live); queue apply really forces
`propagate_family=false` (`codes.rs:176-183`); capabilities are exactly the 0110 four + 0111
six (`capabilities/default.json`, 10 allow-*); `ReviewDocStub` on `/matters/:id/review/:docId`
(`ui/src/app.rs:148`); `--coding-pane: #e8eef2` in `tokens.css:28`; `fts_unavailable` kind
exists (`error.rs:33`); no `review_neighbors` anywhere in `crates/` (grep clean) and no zpdf
in any lockfile. Crate pins verify: tauri 2.11.5 / leptos 0.8.20 / leptos_router 0.8.15 in
`Cargo.lock` and confirmed current on crates.io; trunk 0.21.14 in `ci.yml:106`; stable
toolchain `:32`. Registry, deferred rows, ledger tx `91a1b6b4`, and PR history all check out.

But the adversarial dig found a **real behavior conflict the spec does not currently
survive**: `apply_codes`' privilege lifecycle (`ensure_item_privilege_conn`) **forces
`withhold=1`** on any active claim that isn't already `withhold=1`, and **auto-creates** an
`attorney_client` + `withhold=1` claim when privilege coding is applied with no prior
upsert. Both facts collide head-on with §3.3's ordering requirement and DoD-3's
"withhold==false unless the UI set it." Details below — this is the one item that must be
resolved before Implement.

## Findings (B/M/m/O)

No B (no data-loss default change, no client-PST-in-git, no crate-boundary leak in the
plan's own text). Two M.

### M1 — `ensure_item_privilege_conn` overwrites `withhold=false` → spec's own §3.3 ordering produces a withheld claim, and DoD-3 as written fails

This is the crux. Live code, line-level:

- `apply_codes(add_privilege=true)` calls `ensure_item_privilege_conn` in the same txn
  (`matter.rs:4925-4932`).
- `ensure_item_privilege_conn` (`privilege.rs:210-264`): for an **existing** row, it returns
  unchanged **only if** `status is ACTIVE && withhold == 1 && include_on_log == 1`
  (`:228-234`); **otherwise it runs `UPDATE ... SET status=asserted, withhold = 1,
  include_on_log = 1`** (`:237-243`). For **no** row it INSERTs
  `basis = attorney_client, withhold = 1` (`:248-261`), per the doc comment
  "Defaults: status=asserted, withhold=1 ... basis=attorney_client" (`:207`).

Consequences against the spec as written:

1. §3.3 `review_window_apply` says the UI is "required to call `review_upsert_privilege`
   **before** apply when turning privilege on" (DoD-3 repeats it). Follow that order
   literally: UI upserts `withhold=false` + chosen basis → apply adds privilege coding →
   `ensure_item_privilege_conn` sees an ACTIVE row with `withhold=0` → takes the re-assert
   branch → **flips `withhold` back to 1**. End state: privilege-coded **and withheld** —
   the exact poison §2.6 warns about (0113 produce = `responsive AND NOT withheld`), and
   DoD-3's own assertion "`withhold==false` unless the UI set it" fails.
2. Reverse the order (apply first, upsert second) and the end state is right
   (`upsert_item_privilege` honors `withhold=false`, `privilege.rs:538`), but there is an
   interim persisted claim with `withhold=1` / default basis, and a failed subsequent
   upsert leaves privilege-coded + withheld — an unclean intermediate the spec never
   mentions.
3. DoD-3's "Turning privilege **on** without basis → apply **fails** (no membership
   write)" cannot come from matter-core: `apply_codes` **succeeds** without any prior
   privilege row and silently creates the `attorney_client` + `withhold=1` claim. The
   rejection must be a **chrome-host pre-check** before `apply_codes` is called. §3.3
   gestures at this ("Host must not persist...") but never says that matter-core will
   happily write the default claim — an implementer who assumes `apply_codes` validates
   basis will ship the trap.

Fixes to fold (pick one, pin it):

- **(a) Pin the host-side sequence:** `review_window_apply` with privilege turning on must
  (i) pre-check asserted-basis (else `failed`, no write) and (ii) when the claim row
  already exists, add membership **without** triggering the ensure branch's withhold
  rewrite — or simply pin apply-first-then-upsert and add a DoD-3 test that the **final**
  state after both calls is `withhold==false`, with `get_item_privilege` asserted between
  calls to expose the interim.
- **(b) Matter-core behavior change:** make `ensure_item_privilege_conn` respect an
  existing active row's `withhold` instead of normalizing to 1. This touches the shared
  0027 Desk path (behavior change outside this track's lock) — if chosen, it needs its own
  explicit decision note, not a silent fold.

The review flags the ambiguity; `/foldin`/planner picks the resolution. As written, DoD-3
is a coin flip against live code.

### M2 — Family card cap is DTO-only; `list_family_members` hydrates every full `Item` before the cap, and the orphan-family failure mode is unpinned

§3.5's risk mitigation says "Cap 100 members in the DTO; size from `family_sizes`," and §6
frames it as the fix for "60k family SQL." But `list_family_members(family_id)` has **no
limit parameter** (`matter.rs:2637-2649`): it queries **all** rows via full `Item`
hydration (`item_select_sql` + `map_item_row`) and only then can chrome slice to 100. A
1000-member family pays the full hydration inside matter-core regardless of the DTO cap —
the mitigation caps the wire, not the work. Same shape as 0111's m1 (resolved there by
`family_sizes`); the *list* now needs the same honesty. Also: `list_family_members` calls
`get_family` first and **errors (`FamilyNotFound`) if the families row is missing**
(`:2638-2639`), while `family_sizes` never touches the `families` table — so an item with
`family_id` set but no family row yields `family_size` from §3.5 step 1
**and** a hard error from step 2. §3.5 pins neither. Additionally `Item` here is the full
hydration (subject, addresses, digests...) when the card only needs id/parent/path/subject.

Fold: pin one of (a) a small matter-core helper `family_members_thin(fid, limit)` (thin
rows + SQL LIMIT — mirrors `list_review_thin`, no `ReviewListRow` extension), or (b)
chrome catches `FamilyNotFound` → members empty + `family_truncated` copy, full hydration
accepted as a known v1 cost with the cap honest on the wire. Either is one sentence;
right now the "cap 100" reads as if the query is capped when it is not.

### m1 — §3.4 `position` contradicts itself in the exact case it exists for

"`position` = 1-based count of filtered rows with sort key `<=` anchor key, or `0` if the
anchor is not in the filter." The dropped-out anchor (Save & Next on Unreviewed after
coding Responsive — the headline case, DoD-2) is *by definition* not in the filter, so the
second clause zeroes the first whenever the mechanism matters. Pin one semantic, e.g.
`position = COUNT(rows WHERE key <= anchor_key)` always (footer shows "item N of T" from
the next row's position), and drop the 0-clause or reserve it for missing anchor id.

### m2 — Privilege auto-claim + host pre-check must be stated, not implied

Fold-out of M1 (3): §3.3 should explicitly say "matter-core `apply_codes` auto-creates a
default `attorney_client` + `withhold=1` claim when privilege coding is applied without a
prior upsert (`matter.rs:4926`, `privilege.rs:207-261`); the chrome host must therefore
pre-validate basis before calling apply, and must not rely on matter-core to reject." One
sentence in §3.3; it also becomes a required Phase 1 unit test (apply privilege with no
claim, host returns `failed`, `list_item_codes` unchanged, `get_item_privilege` none).

### m3 — Phase 1 fixture omits `insert_family` and the review-set setup order

The 3-item fixture (`itm_0000..0002`, shared `family_id`) requires: `insert_family(kind)`
first (`matter.rs:2596`) — `insert_item` with `family_id` set hard-errors if the family
row is missing (`:2109-2117`); parent must be inserted before children
(`ParentItemNotFound`, `:2098-2100`); and the Unreviewed filter (DoD-2's neighbors test)
needs `ensure_default_review_set` + `in_review=Some(1)` per row (0111 m5 precedent, here
partially pinned — plan Phase 1 has `in_review=Some(1)` but not the family/set ordering).
One line in plan Phase 1: "insert_family → parent → children; ensure_default_review_set
before any in_review writes."

### m4 — `review_upsert_privilege` args omit `include_on_log` (and cleared-status side effect)

`UpsertItemPrivilegeInput` requires `include_on_log: bool` (`privilege.rs:136`). §3.3 args
pinned as `{ root, item_id, basis, withhold, description, status }` — no
`include_on_log`; the host must pick a value (default `true` is the sane pin; say so).
Also note `status=cleared` silently forces `withhold=0`/`include_on_log=0`
(`privilege.rs:542-546`) — relevant only if the UI ever sends a status other than
`asserted`, but §3.3 says "`status` default `asserted`" without restricting values;
pin "window only ever sends asserted; soft-clear is the separate call" to close it.

### m5 — Body conversion details: UTF-8 lossiness and `truncated` detection unpinned

`read_cas_prefix` returns raw bytes with **no truncated signal** (`matter.rs:1713-1727`):
setting `truncated=true` requires comparing `cas_len(digest)` (or a cap+1 read) against
2 MiB — pin the mechanism (cap+1 read, Desk `review_body.rs:203-214` pattern) so DoD-4's
"2 MiB + 1 → truncated==true" has a deterministic path. And "→ UTF-8" should pin
`String::from_utf8_lossy` (CAS bytes are not guaranteed valid UTF-8; silent lossy
replacement is the honest default, but it must be *chosen*, not discovered in review).

### m6 — Copied `html_strip` is a drift fork by design; give it a pin

"`html_strip` Desk-only; Chrome must **copy** the helper" (§2.2/§3.1) — verified
(`html_strip.rs:40`, `p_tags_do_not_merge_words` `:172`). Copying is the right
crate-boundary call, but nothing pins parity: the Desk strip can change (entity table,
block tags) and the chrome copy silently diverges. Cheap pin: Phase 1 adds the exact
Desk unit tests (`:172-193`) to the chrome copy and a one-line comment in both files
pointing at the twin ("mirror of `dedupe-desk/src/html_strip.rs`; update both"). O-level
if you'd rather defer.

## What looks solid

- **The catalog fix is correctly diagnosed and correctly scoped.** Live `codes.rs:59`
  confirms the Bugbot: `review_code_catalog_blocking` opens
  `open_matter_write` unconditionally, then seeds only when active defs are empty
  (`:63-67`) — the write lock is pure waste on the seeded path. The §3.3 fix
  (read-first → write only when empty) is minimal, keeps Desk/Process coexistence, and
  DoD-5's "assert via open_matter_read path when defs exist" is testable as written.
- **Queue apply force-false is real and stays untouched**: `codes.rs:176-183` ignores
  client `propagate_family` and passes `false` hard; spec §3.3 keeps that contract
  explicitly ("Do **not** change that contract") and Phase 1 keeps the regression test.
  Verified rather than trusted.
- **`fts_unavailable` degradation design is grounded twice over**: the kind already exists
  (`error.rs:33`), `IndexMissing`/`LangPackStale` are the two live variants
  (`matter-search/src/error.rs:24,32,52`), and §3.3's "still return the document with null
  neighbors + `neighbors_error`" is the honesty-preserving shape (FTS down must not hide
  the document — matches 0111's precedent that FTS failure is a kind, never total=0).
- **Neighbor math is buildable from verified primitives**: same WHERE/ORDER BY as
  `list_items_filtered_thin` (`matter.rs:3905`, order `(review_order IS NULL), review_order
  ASC, imported_at, path, id`), hit-intersection via `with_fts_hit_temp` temp table
  (`:3964-3979`) rather than a 50k IN-list, and the anchor sort-key compare makes the
  DoD-2 drop-out test (`next_id == itm_0001` after coding `itm_0000` responsive) sound —
  `code_missing eq true` is Not-Exists on `item_codes`, so the coded anchor genuinely
  leaves the Unread set. Spec correctly fences it as "matter-core may add this one
  helper; chrome `connection()` SQL remains forbidden" (§3.4) — mirrors 0111's
  family-size resolution, no `ReviewListRow` extension.
- **`expand_family_units` semantics match §3.5's claim exactly** — parent + all direct
  children + all family-id members, deduped (`matter.rs:5520-5568`); the spec's "not
  `list_family_members` ∩ ... the whole family unit, same as 0027" is the *accurate*
  description, and the preview-vs-apply consistency note (chrome passes expanded ids to
  the existing `review_codes_preview`) is implementable against the live preview
  (`codes.rs:87-129`, `touches_privilege` checks group **or** key — so privilege-group
  additions are caught; confidential is not in the privilege group, so DoD-3's
  `confidential` → `privilege_would_change==0` holds).
- **Single-group conflict rejection + single-clear means the window's radio model is
  enforced server-side**: adding a second responsiveness code in one batch errors
  (`matter.rs:4795-4805`), and a single add deletes the sibling in-group row (`:4898-4907`)
  — so `1`/`2`/`3` on an already-coded item genuinely *replaces*, matching §3.8's
  "replace other responsiveness."
- **Viewer honesty paths are all live APIs, not wishes**: prefix-read (`:1713-1728`),
  `cas_len` (`:1687`), block-aware strip with entity decoding (`html_strip.rs:104-147`),
  no-`innerHTML`/text-node paint (§3.7) consistent with the CSP object carried over
  unchanged from 0110's M1 fix. The "never decode `native_sha256` binary into the
  WebView" line kills the worst plausible shortcut in one clause.
- **Crate/CI/tokens state is exactly as pinned**: tauri 2.11.5 / leptos 0.8.20 /
  leptos_router 0.8.15 verified in `ui/Cargo.lock` **and** on crates.io (max stable
  2.11.5 / 0.8.20 / 0.8.15 — no 3.x / 0.9 stable); trunk 0.21.14 at `ci.yml:106`; stable
  toolchain `:32`; `chrome-ui` present. `--coding-pane` verified at `tokens.css:28`. No
  zpdf anywhere. No `review_neighbors` anywhere. Capabilities = exactly the ten
  (0110×4 + 0111×6) the spec claims.
- **Registry/deferred/ledger hygiene**: conductor.md marks 0112 **Ready — not started**
  (`conductor.md:295,308`), 0117 Proposed placeholder with the three queue Bugbot items
  (`:300`); deferred rows all live where the table says (D-0112 `deferred.md:921`, D-0117
  `:922`, D-0026-03 `:118` partial, D-0026-05 `:120`, D-0027-05 `:131`, D-0030-06 `:166`);
  ledger tx `91a1b6b4` confirmed in ledger search; ledger 0 pending / 0 drift.
- **Declining the Hermes mixed-axis table is right and verified as a real conflict**: `3`
  = Privileged + `p` = ditto would make Needs review keyboard-unreachable and collapse
  privilege into responsiveness — §3.8's decline (wireframe + 0111 orthogonality win) is
  the same call the board-level 0031 decision recorded; the overlay one-liner ("Privilege
  is `p`, not `3`") is good honesty copy.

## Deferred fold-in table

| Row | Live state (`docs/deferred.md`) | Spec disposition | Verdict |
|---|---|---|---|
| **D-0112-review-window** | :921 — open, "0112 Ready" | Absorb / close on Implement | ✅ |
| **D-0117-queue-virtualization** | :922 — Proposed, PR #113 Bugbot, "Do not steal into 0112" | Remain Proposed; §4 out of scope | ✅ |
| D-0026-03 HTML/image body | :118 — open | **Partial** (text + stripped HTML; raster 0114) | ✅ honest partial |
| D-0026-05 last_review_item_id | :120 — polish | Decline (neighbors replace most of it) | ✅ |
| D-0027-03 auto-propagate | :129/:587 — declined previously | Decline; default off | ✅ |
| D-0027-05 coding GUI smoke | :131 | Analog HITL; residual stays | ✅ |
| D-0030-06 markdown notes / D-0030-07 | :166 | Decline; plain text | ✅ |
| D-0031-05 AI prediction / D-0031-08 | open | Prediction slot "AI off"; HITL | ✅ |
| D-0028-01/02, D-0032-01/02, D-0034-02, D-0113, D-0115/0116, D-0110-deny-unic, D-0062, D-0020-01, D-0067, D-0108 | open/parked | Decline / remain / parked | ✅ |
| Hermes 3=Priv table | decline row §9 | Declined with reasons | ✅ |
| AEO / Local-AI / mock retune | not minted | Not minted | ✅ |

No open med/high row overlaps the window surface that this spec misses. Next free ID
**0118** stands (0117 minted).

## Cursor / last-PR comments the plan missed

`gh`: last four merged = **#114, #113, #112, #111** — matches §2.8 exactly. PR #113:
**0 human comments**; one Bugbot review with **exactly 4 inline items**, all on
commit `72708ed`, all verified live today on `main`:

1. `codes.rs:71` catalog exclusive write lock — **folded here** (§3.3 + DoD-5). Correct.
2. `queue.rs:748` header breaks virtualization spacer — minted 0117. Correct.
3. `queue.rs:705` empty page misreported vacant — minted 0117. Correct.
4. `queue.rs:331` arrow keys leave `visible_range` — minted 0117. Correct.

Nothing in the Bugbot set touches the window surface; §2.8's dispositions are complete.
No new placeholder needed; 0118 stands.

## Research / tools notes

- **ai-brains: used** from `C:\dev\Dedupe` — preflight inited, **3892** pinned (spec §2.5
  says 3891; +1 self-correcting drift, matches 0111's 3888→3890 pattern); `sync query`
  recovered decision `10a4067c` (0112 Ready) whose content matches the spec's product
  locks verbatim (radios ⊥ privilege, Hermes decline, ditto snapshot, 2 MiB prefix,
  copied strip, propagate defaults, sort-key neighbors, catalog read-first, 0117 mint,
  SCHEMA 39); semantic recall surfaced 0031/0026/0027 decisions consistent with the
  spec's lineage claims.
- **ledgerful: used** from `C:\dev\Dedupe` — doctor readyForPublish **true** (standing
  warns only: phantom-promote legacy, sig-pin, completion-unreachable, gemini optional);
  `ledger status --compact` **0 pending / 0 unaudited drift**; ledger search confirmed
  tx **`91a1b6b4`** (conductor/0112-ReviewWindow, Docs, 2026-08-30 02:25). `scan
  --impact`: dirty tree is conductor registry + deferred lines only — **LOW**, no
  product crates.
- **Online research: applied** — crates.io live API: `tauri` max stable **2.11.5** (no
  3.x), `leptos` **0.8.20**, `leptos_router` **0.8.15** — all three match the spec's
  plan-time pins and the workspace `Cargo.lock`s; §2.4's "re-verify at execute" table is
  current as of this review. MS-PST: N/A per spec — confirmed nothing in the window
  touches PST parsing.
- **PR/PR-comments**: `gh` verified (1 Bugbot review, 0 human comments, 4 items —
  listed above).

## Verdict: Ready after fixes

No B. Fold these in before Implement (all small; M1 decides an ordering, not a feature):

1. **M1** — resolve the privilege lifecycle conflict: pin the host sequence such that the
   final persisted state is `privilege`-coded with `withhold==false` (apply-order either
   way, with an explicit interim-state stance), **and** state that matter-core's
   `apply_codes` auto-creates `attorney_client`+`withhold=1` when no claim exists — the
   host pre-check (m2) is mandatory, matter-core will not reject. If you instead change
   `ensure_item_privilege_conn`'s normalize-to-1 behavior, that is a Desk-shared behavior
   change and needs its own decision note.
2. **M2** — pin the family-card mechanics: either a thin/limit matter-core helper or an
   explicit "full hydration, DTO-cap, orphan → catch `FamilyNotFound`" statement. The
   current "cap 100 in DTO" reads as if the query is capped.
3. **m1** — fix §3.4 `position`'s contradictory 0-clause (dropped-out anchors are the
   mechanism's headline case).
4. **m3** — Phase 1 fixture: `insert_family` → parent → children →
   `ensure_default_review_set` ordering; `in_review=Some(1)` stays.
5. **m4/m5/m6** — add `include_on_log` default + asserted-only status; pin
   cap+1-read for `truncated` + `from_utf8_lossy`; pin copied-strip parity tests.

`/foldin 0112` folds this file into spec/plan (fold review files only; do not implement here).