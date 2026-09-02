# 0124 — Review queue chrome (rail, columns, no colliding text)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do **not** rewrite **0117** `visible_range`
> math, **0118** window fetch guards, **0120** overlay draw, **0125** produce
> canvas, or **0126** Process jobs table. Do not vendor `C:\dev\dedupe-frontend`.
> Do not mint a BCC-default track. Do not steal **0100–0104**.

- **Track ID:** 0124-ReviewQueueChrome
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes first-pass queue. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-01); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is **layout research only** (244px rail, nowrap cells). Do not port Archivo, coral, REDACT/WITHHOLD privilege pills, or fake `ACME0001` Control#.
- **Status:** Completed
- **Depends on:** **0111 / 0117 Completed** · **0123 Completed** (PR **#139** / `fce416e`) · schema **v41** (no bump)
- **Spec authored:** 2026-09-01 (placeholder → Ready)
- **Series:** T (mockup chrome fidelity)
>
> **Closes / absorbs:** `D-0124-review-queue-chrome` (this track). Does **not** close D-0125–D-0126, D-0028-02, D-0110-deny-unic, D-0062-codesign.
> **HITL:** owner launches the **release** chrome EXE on a synthetic matter with a long Exchange X500 `from_addr` and a queue **>500** rows: From/Subject must **ellipsis**, never paint on Fam/Resp/PRIV; extras grid also clips. After Next, StatusBar left shows a truthful `Rows 501–… of N`. Enter / row click still opens **0112**. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-09-01):** PRs **#140, #139, #138, #137**. Disposition in §2.8. No new mint. Next free ID **0127**.
>
> **Harness fold-in (2026-09-01):** `opencode-review.md` + `agy-review.md`. Slot plumbing is **context** (`QueueChromeCtx` from `wrap_review`), not children props. Viewport `client_height` is measured on **mount + resize**, not only `on:scroll`. Control# `title` stays the item id. See §2.9 / §9.
>
> **Owner locks (inherit 0123):** IBM **Plex**, action **ink-navy `#1b3049`**, cool paper, red = privilege / withhold / blocker / overlay only. Privilege first-pass column stays **PRIV** coding (0111). **0123** 46/30 shell stays; this track **fills** the reserved right TopBar slot and StatusBar left — it does not restyle the shell chrome.
>
> **Stack lock:** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. `ui/` stays workspace-excluded. No daemon. No schema bump. 0117 `ROW_HEIGHT == 32` / `OVERSCAN == 8` unchanged; `PAGE_LIMIT == 500` stays in `pages/queue.rs`. 0119 latch / 0121 OPT / 0122 Busy unchanged.

---

## 1. Objective

Make the first-pass queue **readable and selectable** without lying about codes: cells **ellipsis** instead of painting on the next column (owner HITL: X500 From into Fam/Resp/PRIV), a 244px rail, a toolbar titled as the active queue, a bulk bar that keeps 0111 privilege preview, and a StatusBar left slot with a truthful SQL-page row range.

Colliding columns are **correctness**: counsel can tag the wrong document if From sits on Privilege. Unique-export is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0123 Completed** (PR **#139** / `fce416e`) shipped the shared shell with an empty `.right-slot` and `.status-left`. **0117 Completed** fixed virtualization honesty. Live queue is still a **single pane** of chips + a 640px viewport whose grid cells have **no** ellipsis (`app.css` `.queue-row`).

### 2.2 Live APIs (plan-time 2026-09-01, HEAD `50b303d`; re-verify at execute)

| Surface | Fact |
|---|---|
| `schema.rs` | `SCHEMA_VERSION == 41`. **No bump.** |
| `ui/styles/app.css` `.queue-row` | Tracks `32px 72px 110px 140px minmax(160px, 1fr) 56px 48px 72px` (extras adds four). `.queue-viewport { height: 640px; overflow-x: hidden }`. **No** `min-width: 0` / `text-overflow: ellipsis` on cells. Grid items default `min-width: auto` → long `from_addr` overflows. |
| `ui/src/queue_window.rs` | `visible_range`, `ROW_HEIGHT = 32`, `OVERSCAN = 8`, empty-page clamp, `scroll_top_to_reveal`. **Do not rewrite.** `PAGE_LIMIT` is **not** here — it lives in `pages/queue.rs` (`const PAGE_LIMIT: u64 = 500`). |
| `ui/src/pages/queue.rs` | `PAGE_LIMIT == 500`. Chips Unreviewed / Privileged / Responsive + saved-search chips. Bulk **Tag…** + `review_codes_preview` privilege confirm. Footer `{n} selected · {total} in queue`. Enter / click → `review_doc_href`. `control_number` = `review_order`. PRIV pill vs `—`. Extras Produced cell is hardcoded `"—"`. From/Subject spans have **no** `title` (Control# uses `title=item id` — **keep** that). `viewport_h` inits `640.0` and is written **only** in `#queue` `on:scroll`. Own `root_sig` from route params (separate from `MatterShellCtx.root`). `last_fetch_meta: (offset, total, fetched)`. |
| `ui/src/shell.rs` | `MatterShell` + empty `<div class="right-slot">` and `<div class="status-left">`. `REVIEW_FLAG` already on the right. `MatterShellCtx` = `root` / `overview` / `error` (parent→child). Fill slots; do not change 46/30 or flags’ **meaning**. |
| `ui/src/app.rs` wraps | `wrap_review` mounts `<ReviewQueue/>`; `wrap_review_window` mounts `<ReviewWindow/>`. **Both** use `WorkspaceTab::Review`. Slots/Ctrl+K `#queue-goto` are **queue-route only**, not tab-keyed. |
| `src/queue.rs` `QueueRow` | `review_order`, `from_addr`, `subject`, `parent_item_id`, `privilege_coded`, `withhold`. **No** Bates / SMTP-separate / produced count. `ReviewListRow` in matter-core matches — no Bates column. |
| `matter-core` `FilterSpec` | `subject` `contains` compiles. `review_order` is a **thin column name**, not a proven FilterSpec condition — **do not** mint a new filter field this track. Nested OR stays **D-0028-02**. |
| Mock `pages/queue.rs` | 244px `.panes-2`; `.doc-table td { white-space: nowrap }`. Privilege REDACT/WITHHOLD/CANDIDATE — **do not port**. Fake QC glyphs — **do not port**. |
| Ctrl+K (`app.rs`) | Focuses `#matter-search` on the list; else `#ctrl-k-hint` no-op. Review has no Go-to id yet. |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only)

Steal **layout**: 244px rail, nowrap+ellipsis (chrome is a windowed grid, not mock’s scrolling table), Go-to in the 0123 right slot, row range in StatusBar left. Do not steal mock privilege pills or ACME Control#.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2** | Reject 3.x. |
| `leptos` / `leptos_router` | **0.8.x** CSR | No major bump. |
| Schema | **41** | No bump. |
| Rust | **stable** | No nightly. |
| trunk | **0.21.14** | Keep. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; plan-time **4597**; fold-in **4603**).
- Recall: 0111 PRIV / `review_order` Control# / 32px overscan 8 (`6a773081`); 0117 keep `visible_range` (`50db2cfe`); 0123 fills slots later (`dc76177b`).
- `ledgerful doctor --json` `readyForPublish: true`; warns phantom-promote, completion-unreachable, sig-pin, sig-version, impact-stale (refreshed this pass).
- Ledger compact: **0 pending / 0 unaudited drift**. Execute starts `0124-review-queue-chrome` **FEATURE** after owner git-commits this plan if still dirty.
- `ledgerful scan --impact` **LOW** on stray untracked (`.claude`, repo-root `agy-review.md`, `fixtures/keep_set_summary.json`). Do not `git add` those. Federated `output/` 5000-file budget — ignore. Hotspot: `queue.rs` (edit carefully; do not touch 0117 math).

### 2.6 What we could not verify

Owner HITL long X500 + Next-page range on the release EXE. Execute re-reads FilterSpec if anyone added `review_order` eq since this pin.

### 2.7 Related deferred (roll)

See §9. Absorb **D-0124**. Remain D-0125–D-0126, D-0028-02. Decline D-0032-08 / D-0020-01.

### 2.8 Last-PR Cursor comments (2026-09-01)

PRs **#140, #139, #138, #137**. Inline comments **0**. Issue comments Bugbot **usage-limit** only. **Decline**.

| Origin | Verdict |
|---|---|
| #139 / #140 MatterShell | **Remain 0123** (Completed). Fill reserved slots only. |
| #137 Process Busy | **Remain 0122**. Do not steal. |
| #140–#137 usage-limit | **Decline**. |

No new mint. Next free ID **0127**.

### 2.9 Product locks (do not invent at execute)

**Collision (DoD-1 — first).** Do **not** wrap (breaks 32px virtualization). Every `.queue-header` / `.queue-row` cell (`[role=columnheader]`, `[role=gridcell]`), default **and extras**:

- tracks: `minmax(0, …)` (or `min-width: 0` on the cell) so CSS grid cannot grow past the track
- `overflow: hidden; text-overflow: ellipsis; white-space: nowrap`
- `title` on From / Subject / Custodian = the **display** string (full X500 / subject / custodian). Control# **keeps** `title=item id` (today’s hover); do not replace it with the review-order string (that text is already the cell)

HITL: a 200-char X500 From never paints on Fam/Resp/PRIV.

**`visible_range` frozen.** Do not edit `queue_window.rs` formulas, `ROW_HEIGHT`, `OVERSCAN`, header-outside-spacer, empty-page clamp, or arrow `scrollTop`. Do not change `PAGE_LIMIT` in `pages/queue.rs` (500). Viewport may become flex/`1fr` so `client_height` reflects the 0123 remaining pane (drop magic `640px` if it overflows the shell). **Required if CSS height is no longer 640:** measure `#queue.client_height` on **mount and resize** (page effect / `on:mount` / `ResizeObserver`) and write `viewport_h` **before** the first `visible_range`. Do **not** leave `viewport_h` at the init `640.0` until the first `on:scroll` — that breaks arrow `scroll_top_to_reveal` on a shorter flex pane. Formulas in `queue_window.rs` stay untouched.

**Honesty columns.**

- Control# = `review_order` (string of the int, or `—`). **Never** fake `ACME0001`. Do **not** call `latest_control_number` unless it is already on `QueueRow` (it is **not**, plan-time). Produced extras cell stays `"—"` until a real field exists — do not invent Image/Native.
- Privilege = **PRIV** pill vs `—`. Withhold stays extras. Do not port mock REDACT/WITHHOLD/CANDIDATE into the first-pass Privilege column.
- No fake QC glyph counts.

**From display.** Pure helper (unit-tested): if `from_addr` contains `@`, show it; else show the stored string (X500) with ellipsis + `title`. Do not parse `/O=` into a guessed SMTP.

**Family members.** If `parent_item_id` is Some and date/from/subject are empty: copy from the parent **if that parent is in the current SQL page**; else show `"— attachment"` (subject cell). Do not invent Bates.

**Rail (244px).** Two-pane: `244px 1fr`. Live filters stay:

| Rail row | Maps to | Count |
|---|---|---|
| Unreviewed (default) | `preset_uncoded_json` | `page.total` when that chip is active; else optional saved-search style prefetch **or** omit until selected |
| Privileged | `preset_privilege_json` | same |
| Responsive | `preset_responsive_json` | same |
| Saved searches | existing `saved_search` rows | existing `saved_totals` |
| Needs decision / Redaction QC / Consistency | **inert**, count **0**, no click that loads an empty corpus as “done” | honest “no filter yet” |

Do **not** invent FilterSpec nested OR (D-0028-02). Move saved-search **chips** off the Unreviewed row into the rail.

**Toolbar.** Title = active queue name + `{total} docs`. Keep keyword (`#queue-keyword`, `/` focuses it). Include-family stays. Lead/QC extras stay a Columns/Lead control (checkbox OK). Save control label **Save search**; page `aria-label` is the queue title so Save is not the accessible name.

**Go-to (0123 `.right-slot`, queue route only — not the review window).** Input `#queue-goto`. Ctrl+K focuses it when `#matter-search` is absent **and** `#queue-goto` is in the DOM (queue route). `wrap_review_window` must **not** mount `#queue-goto` (tab is also `Review`). Use the **page’s** `root_sig` for navigation (do not mix `MatterShellCtx.root` with the page fetch root). Behavior:

- Integer → match `review_order` on the **current SQL page**; if found, set current row / open; if not, show an inline miss using `last_fetch_meta`, e.g. `Control# 850 not found in current page (Rows 1–500)` — **not** a silent no-op and **not** “document does not exist.” Do not add a new FilterSpec field.
- Else → `subject` `contains` via **existing** FilterSpec (reset offset 0) or current-page substring; empty result is an honest empty queue, not `total=0` vacancy lie (0117).
- Bates string that is not a `review_order` int: same miss / subject path — **no** fake Bates index.

**Bulk.** Keep Tag… + privilege-change preview (0111). Add **Select page ({len})** where `{len}` is `p.rows.len()`; populate `selected` with those ids only. Label must say **page**, not corpus / matching. Staging / Privilege QC / batch: omit or disabled with “not this track”.

**Row range (0123 `.status-left`, queue route only).** `Rows {offset+1}–{offset+fetched} of {total}` when `fetched > 0`; empty page uses 0117 copy (not “0 in queue” when `total > 0`). Keep `{n} selected` visible (footer or left slot). Shortcut hint may stay in `?` help.

**Shell API (pinned — Leptos context is parent→child only).** Do **not** pass Go-to / range as `MatterShell` children from `wrap_review` (those closures cannot see `last_fetch_meta` / `p.rows` / `current_idx` inside `ReviewQueue`). Pin:

1. `wrap_review` **provides** a `QueueChromeCtx` (or extend `MatterShellCtx` with optional queue fields) **above** `MatterShell`: writable `queue_range` (from `last_fetch_meta`) and `goto_request` (string / submit). `ReviewQueue` writes range + consumes Go-to. Slot UI in `TopBar` / `StatusBar` **reads** that context when present.
2. `wrap_review_window` does **not** provide the ctx; slots stay empty.
3. Other workspaces stay empty. Do not change `REVIEW_FLAG` text, Admin span, or 46/30 grid.

---

## 3. In scope

`ui/src/pages/queue.rs`, `ui/styles/app.css` queue/rail rules, small `shell.rs` / `app.rs` slot+Ctrl+K wiring, pure helpers + `#[cfg(test)]` in queue.rs (and CSS `include_str` lock). Host `src/queue.rs` **only** if a display field is already on `ReviewListRow` (do not add Bates).

### 3.1 Collision + From helper

CSS + `display_from` / `family_cell_text` unit tests.

### 3.2 Rail, toolbar, Go-to, bulk page-select, StatusBar range

`QueueChromeCtx` from `wrap_review`; mount/resize `viewport_h`; **Select page ({len})**; Go-to miss copy with current range.

---

## 4. Out of scope (do NOT do here)

- `visible_range` / overscan / page-limit rewrite (**0117**).
- Review window async / apply sequence (**0118**). Image overlay (**0120**).
- Produce canvas (**0125**). Process jobs table (**0126**).
- Nested saved-search OR (**D-0028-02**). New FilterSpec `review_order` op.
- Fake Bates, mock privilege pills, QC warning glyphs, select-all-matching **corpus**.
- Schema bump, BCC-default, Archivo/coral, daemon.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** **0111, 0117, 0123 Completed**. Schema **41**.
- *Verified to date:* no cell ellipsis; shell slots empty; Enter opens 0112 (HEAD `50b303d`).

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Ellipsis without `min-width: 0` still overflows | Both track `minmax(0,…)` **and** cell overflow; extras grid in tests/HITL. |
| Go-to invents a filter field | Current-page `review_order` + existing subject contains only. |
| Slot children cannot see page signals | `QueueChromeCtx` from `wrap_review`; window route does not provide it. |
| Flex viewport + `viewport_h` stuck at 640 until scroll | Mount + resize measure `#queue.client_height` before first `visible_range`. |
| Rail “Needs decision” looks like an empty review set | Inert + count 0 + “no filter yet”. |
| `queue.rs` hotspot + 0117 regressions | Do not touch `queue_window.rs`. Keep 0117 host/ui twin tests green. |
| Ctrl+K steals list search | `#matter-search` still wins when mounted; else `#queue-goto` (queue route only) then hint. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — No collision:** Long From/Subject never overlap Fam/Resp/Privilege (ellipsis + display `title` on From/Subject/Custodian; Control# `title` stays item id; 32px rows). Extras columns clip. CSS/helper tests. `ROW_HEIGHT` / `visible_range` unchanged.
- [ ] **DoD-2 — Rail + toolbar:** 244px rail; Unreviewed still `preset_uncoded`; saved searches in the rail not as sibling chips; title = active name + count; Save search is not the page accessible name. Inert zero-rows are not a vacant-corpus lie.
- [ ] **DoD-3 — Go-to + range + bulk:** `#queue-goto` via `QueueChromeCtx` on the **queue** route only (`wrap_review_window` empty). Ctrl+K focuses `#queue-goto` when mounted. Integer miss copy names Control# **and** current `Rows a–b`. StatusBar left shows truthful SQL-page range after Next. **Select page ({len})** + Tag… + privilege preview remain. Mount/resize `viewport_h` if CSS is not 640. Enter / row click still open 0112.
- [ ] **DoD-4 — Hygiene:** No `unwrap`/`expect` in new production code. No schema bump. 0117 twin tests + 0122 Busy tests still pass. Plex / `#1b3049` / no Archivo/coral. `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` + `cargo test -p dedupe-chrome`. trunk chrome-ui builds.
- [ ] **DoD-5 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased; `D-0124-review-queue-chrome` closed; ledger committed (`FEATURE`). **0125–0126** stay Proposed unless separately implemented.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml
cargo test -p dedupe-chrome
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Do **not** `git add` operator PSTs, `output/`, stray `agy-review.md`, or `fixtures/keep_set_summary.json`.

---

## 9. Deferred absorb / decline

| ID | Disposition |
|---|---|
| **D-0124-review-queue-chrome** | **Absorb — this track.** |
| **D-0123-matter-shell** | Closed in **0123**. Fill slots only. |
| **D-0125-produce-canvas** | Remain (**0125**). |
| **D-0126-process-chrome-visual** | Remain (**0126**). |
| **D-0028-02** | Remain (nested OR). |
| **D-0117-queue-virtualization** | Closed in **0117**. Do not reopen math. |
| **D-0110-deny-unic** | Remain (upstream). |
| **D-0032-08** / **D-0020-01** | Decline (operator GUI smoke). |
| **D-0062-codesign** | Remain. |
| Bugbot usage-limit on #137–#140 | **Decline**. |
| Mock REDACT/WITHHOLD / ACME Control# | **Decline**. |
| BCC-default | Never. |
| opencode-M1 | **Fold** — `QueueChromeCtx` from `wrap_review`; not shell children props. |
| opencode-M2 | **Fold** — mount + resize `client_height` if viewport is not 640px. |
| opencode-m1 | **Fold** — slots by **route** (`wrap_review_window` empty), not `WorkspaceTab::Review`. |
| opencode-m2 | **Partial** — pin count 4603; owner git-commit Ready docs before FEATURE. |
| opencode-O1 | **Fold** — Control# `title` stays item id; From/Subject/Custodian get display titles. |
| opencode-O2 | **Fold** — `PAGE_LIMIT` is `pages/queue.rs`, not `queue_window.rs`. |
| opencode-O3 | **Fold** — Go-to uses the queue page `root_sig`. |
| agy-M1 | **Already covered** — DoD-1 ellipsis + `min-width: 0`. |
| agy-M2 | **Fold** — label **Select page ({len})**; ids = `p.rows` only. |
| agy-M3 | **Fold** — miss copy `Control# N not found in current page (Rows a–b)`. |
| agy-m1 / agy-m2 | **Already covered** — inert rail; `display_from` no X500 parser. |
| agy-O1 | **Already covered** + opencode-M2 measurement. |

---

## 10. Unblocks

Counsel can first-pass without X500 From covering Privilege. **0125** / **0126** stay independent page guts on the 0123 shell.
