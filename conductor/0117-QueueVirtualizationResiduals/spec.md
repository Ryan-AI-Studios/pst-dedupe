# 0117 — Queue virtualization residuals (PR #113 Bugbot)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export, matter-home
> (**0110**), first-pass product (**0111**), three-pane coding (**0112** / **0118**),
> produce (**0113** / **0119**), zpdf (**0114** / **0120**), TIFF/OPT (**0115** /
> **0121**), or Process (**0116** / **0122**). Do not vendor `C:\dev\dedupe-frontend`.
> Do not mint a BCC-default track.

- **Track ID:** 0117-QueueVirtualizationResiduals
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes first-pass queue. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-31); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (density, not coral, not 13-col lead/QC default).
- **Status:** Completed
- **Depends on:** **0111 Completed** (PR **#113** / `3c4ca65`) · schema **v41** (no bump)
- **Spec authored:** 2026-08-31 (placeholder → Ready)
- **Series:** O (Review chrome) — PR #113 queue Bugbot residual
>
> **Closes / absorbs:** `D-0117-queue-virtualization` (this track). Does **not** close D-0118–D-0122, D-0110-deny-unic, D-0026-01 Desk residual, D-0020-01 operator smoke, D-0062-codesign.
> **HITL:** owner launches the **release** chrome EXE against a **synthetic** matter with a windowed Unreviewed page. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-08-31):** PRs **#124, #123, #122, #121**. Disposition in §2.8. Three **0116** Process/Produce Bugbot items **not** this track: cancelled-produce → **0119**; extract-all Busy + orphan snapshot → **minted 0122**.
>
> **Review fold-in (2026-08-31):** `opencode-review.md` + `agy-review.md`. Disposition in §2.10 and `foldin-note.md`.
>
> **Stack lock (inherit 0110–0116):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / blocker only. No daemon. No schema bump. `ui/` stays workspace-excluded. One pipeline.

---

## 1. Objective

Make the **0111** first-pass queue **honest under keyboard and after bulk tag**: the column header must not steal spacer math, an empty *page* must not be shown as an empty *corpus*, and arrow keys must keep the current `.queue-row[aria-selected]` mounted inside the DOM window.

This is **correctness**, not chrome polish. Counsel tagging Unreviewed on a later page, or arrowing a 500-row fetch, must not lose the selected row or be told the queue is vacant while `total > 0`. Unique-export is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0111 Completed** (PR **#113** / `3c4ca65`) shipped the virtualized queue. Three **valid** Cursor Bugbot findings were parked here so **0112** could proceed. **0116 Completed** (PR **#123** / `727c857`). This `/plan-track 117` expands the placeholder.

### 2.2 Live APIs (plan-time 2026-08-31, HEAD `3bde470`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 41`. **No schema bump this track.** |
| `ui/src/pages/queue.rs` | Header **inside** translated `.queue-window` (~732). Empty branch `total == 0 \|\| fetched_len == 0` → `"0 in queue"` (~703–704). `ArrowDown`/`ArrowUp` bump `current_idx` only (~318–328); never `scroll_top` / DOM `scrollTop`. |
| `ui/src/queue_window.rs` + host `src/queue_window.rs` | `visible_range`; `ROW_HEIGHT = 32`; `OVERSCAN = 8`. Host tests: span ≤ viewport_rows + 2×overscan. **Keep formula.** Duplicate UI copy must stay in sync. |
| `PAGE_LIMIT` | **500** in `queue.rs`. SQL page is 0111 lock; do not raise toward 50k. |
| Footer | Already `{selected} selected · {p.total} in queue` — honest. The **body** empty copy is the liar. |
| CSS `app.css` | `.queue-header { position: sticky; top: 0 }` **inside** a `transform` parent — sticky fails (containing block). Height 32px matches `ROW_HEIGHT`. |
| Host `review_queue_page` | Unchanged this track. No `connection()` in chrome. |
| `review_code_catalog` | **Already** `open_matter_read` first (`codes.rs` ~94–107). PR #113 catalog write-lock was **0112**. Do not re-open. |
| CI | `chrome-ui`: wasm32 + `trunk` + `cargo test -p dedupe-chrome`. Keep it. |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only)

Steal density only. Do **not** copy coral, lead/QC-as-default, or REDACT/WITHHOLD as the Privilege column (0111 locks).

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2** (`Cargo.toml`; lock 2.11.x as of 0116) | Reject **3.x / pre-release**. |
| `leptos` | **0.8.x** CSR | Do not bump major. |
| Schema | **41** | No bump. |
| Rust | **stable** (CI) | No nightly. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 4128 pinned).
- Recall: 0111 Ready locked 32px / overscan 8 / no infinite-scroll; 0112 minted this ID; 0116 Completed leaves 0117 Proposed.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` **0 pending / 0 unaudited drift** before this tx. Doctor: phantom-promote, impact-stale, sig-pin, sig-version — none block planning.
- Ledger tx for this planning pass: `a61421cf-cb52-4697-aee1-7664c2ddf2f2`.
- `scan --impact` LOW (docs/conductor expected). Hotspot #1 is `queue.rs` — stay inside that file + `queue_window` + CSS.

### 2.6 How this advances the north star

First-pass coding is the counsel loop. A vacant-corpus lie after bulk tag, or an unmounted `aria-selected` row, is a **silent drop of the current document** — the same honesty class as unique-export (no silent drops). Not UI chrome for its own sake.

### 2.8 Last-PR Cursor comments (mandatory)

Last four merged product PRs: **#124** (docs 0116), **#123** (0116 Process fold), **#122** (docs 0115), **#121** (0115 TIFF/OPT).

| PR | Surface | Disposition |
|---|---|---|
| **#124** | docs registry | Bugbot usage-limit comment only; **no** inline findings |
| **#123** | Process + Produce | **Three valid Bugbot items** — not queue. **Cancelled produce as success** → fold sketch into **0119**. **Extract-all Busy wipes queue** + **live jobs shown as orphans** → **minted 0122**. Do not steal into 0117. |
| **#122** | docs registry | no review/issue/line comments |
| **#121** | ImageOptFactory | Already **0121**. Do not steal. |

PR **#113** (this track’s origin; not in the last-four window) — three queue comments still live:

| Bugbot id | Severity | Fold |
|---|---|---|
| `7e063d89` Header inside spacer | Medium | **This track** §3.1 |
| `09489de5` Empty page vacant | Medium | **This track** §3.2 |
| `7fdba78c` Keyboard leaves window | Medium | **This track** §3.3 |
| `dbf432d2` Catalog write lock | Medium | **Already fixed** in 0112 (`open_matter_read` first). Decline. |

No BCC-default track. Next free ID after mint: **0123**.

### 2.9 Product locks (do not invent at execute)

See §3. Inherit 0111: Unreviewed default; Privilege column is coding not WITHHOLD; `propagate_family` false on queue apply; `fts_unavailable` is not `total=0`.

### 2.10 Review fold-in (2026-08-31)

Sources: `opencode-review.md`, `agy-review.md`. Inputs not edited.

| Id | Sev | Disposition | Lock |
|---|---|---|---|
| opencode-M1 | Major | **Agree — fold** | Clamp via **one Effect** calling `offset_after_empty_page`; write only when `Some(new) != offset`. Never clamp inside the render closure. Gap/clamp refetch: **keep last good `page`** until the next OK (do not `page.set(None)` on empty-page). Gap: error banner + last rows; pager stays usable; **no** auto-`offset.set(0)`. |
| opencode-M2 | Major | **Agree — fold** | `reset_queue_navigation()`: chips, keyword Enter, family toggle, saved-search chips, pager — all reset `current_idx`, `scroll_top` signal, **and** `#queue.scrollTop`. |
| agy-M1 | Major | **Agree — partial** | Windows classic scrollbar: `.queue-header` and `.queue-viewport` both `overflow-y: scroll; scrollbar-gutter: stable` so the 1fr Subject column lines up. Not padding-right guesses. |
| agy-M2 | Major | **Already covered** | §3.3 already requires signal + DOM `scrollTop`. |
| agy-M3 | Major | **Already covered** | Write-only-when-changed + gap ≠ clamp; routing tightened by opencode-M1. |
| opencode-m1 | Minor | **Agree — fold** | Shift+ArrowDown stays open-current (0111); not selection-extend. |
| opencode-m2 | Minor | **Agree — fold** | No ui page-snapshot harness; header-outside is HITL + `review.md`. |
| opencode-m3 | Minor | **Agree — partial** | Host `include_str!("../ui/src/queue_window.rs")` parity with host `queue_window.rs` (path from `src/`, not `../../ui`). |
| opencode-m4 | Minor | **Agree — fold** | Gap copy stays until the user paginates; do not auto-reset offset. |
| opencode-m5 | Minor | **Agree — fold** | Write DOM `scrollTop` **only** when `scroll_top_to_reveal` differs from current `scroll_top`. |
| agy-m1 | Minor | **Already covered** | §3.2 already clamps `current_idx` after shrink. |
| agy-m2 | Minor | **Decline** | `visible_range` already `floor`/`ceil`; formula stays locked. |
| agy-O1 | Opportunity | **Decline** | PageUp/PageDown would expand keyboard scope. |
| opencode-O1 / O2 | Opportunity | **Decline** | Pin-count / doctor-line cosmetic. |

---

## 3. In scope

UI + CSS + shared `queue_window` helpers only. **Do not** change `review_queue_page`, FilterSpec, coding apply, or Process/Produce.

### 3.1 Header outside the translated body (`7e063d89`)

- Column header is **not** a child of the `transform:translateY` `.queue-window`.
- Preferred layout: `.queue-grid` (optional `.extras`) wrapping a **static** `.queue-header` **sibling above** `.queue-viewport` (`#queue`). Rows-only spacer inside the scroller. Sticky-inside-transform is **forbidden**.
- Spacer height remains `fetched_len * ROW_HEIGHT` (**rows only** — do not add 32px for the header).
- `visible_range` still uses the **viewport** `client_height` (row scroller). Header must not consume that height (hence sibling-above, not sticky-inside-scroller).
- `.extras` grid template applies to **both** header and rows from one parent class.
- Windows classic scrollbar (WebView2): `.queue-header` and `.queue-viewport` both use `overflow-y: scroll` and `scrollbar-gutter: stable` so the 1fr Subject column (and columns after it) line up with the header. Do **not** guess `padding-right: 17px`. Overlay scrollbars: gutter is a no-op (acceptable).
- Do **not** change `visible_range` formula except that header is not a row.

### 3.2 Empty page is not empty corpus (`09489de5`)

- `"0 in queue"` **body** copy **only** when `p.total == 0`.
- Clamp is a **pure decision consumed by exactly one Effect** (not the render closure, not the fetch Effect). That Effect reads `page` + `offset`, calls `offset_after_empty_page`, and writes `offset` **only when** `Some(new) != offset.get()`.
- When `fetched_len == 0 && total > 0 && offset >= total`: clamp to last page `(total.saturating_sub(1) / PAGE_LIMIT) * PAGE_LIMIT` then the existing fetch Effect re-fires.
- When `fetched_len == 0 && total > 0 && offset < total`: **data gap** — error banner, **keep last good `page` rows** (do not `page.set(None)`). Pager Prev/Next stay usable. Copy stays until the operator navigates. **Do not** auto-`offset.set(0)`.
- While a clamp/gap refetch is in flight: keep the last good page mounted (`loading` may show); only replace `page` on a successful fetch or a real command error (invalid filter, `fts_unavailable`, …).
- After a successful fetch with rows: clamp `current_idx` to `fetched_len.saturating_sub(1)` if it would be past the page (bulk tag shrink).
- Footer keeps using `p.total`.

Pure helpers (host `queue_window.rs` **and** UI twin):

- `last_page_offset(total, page_limit) -> u64`
- `offset_after_empty_page(offset, total, fetched_len, page_limit) -> Option<u64>` (`Some` = clamp)

### 3.3 Keyboard keeps the current row mounted (`7fdba78c`)

- `ArrowDown` / `ArrowUp` (and Space/Enter that use `current_idx`) must keep `current_idx` inside `visible_range(scroll_top, viewport_h, ROW_HEIGHT, fetched_len, OVERSCAN)`.
- **Shift+ArrowDown** stays **open-current** (0111); this track does **not** turn it into selection-extend.
- Pure helper `scroll_top_to_reveal(idx, row_h, viewport_h, scroll_top) -> f64`: if the row is above the window, snap to `idx * row_h`; if below, snap so the row sits on the bottom edge; else unchanged.
- **Write both** the `scroll_top` signal **and** the `#queue` element’s `scrollTop`. Signal-only does not move the scroller. Write the DOM **only when** the helper return differs from current `scroll_top` (Enter/Space on an already-visible row 0 must not force a scroll).
- Centralize `reset_queue_navigation()`: set `current_idx = 0`, `scroll_top` signal `0`, and `#queue.scrollTop = 0`. Call it from pager Prev/Next **and** every chip / keyword-Enter / include-family / saved-search click that today sets `offset = 0` + `current_idx = 0`.

### 3.4 Tests (normative)

Host (`cargo test -p dedupe-chrome`):

1. Existing `visible_range_*` tests still pass (formula unchanged).
2. `last_page_offset`: total 0 → 0; total 500 / limit 500 → 0; total 501 / limit 500 → 500.
3. `offset_after_empty_page`: `(500, 400, 0, 500) → Some(0)`; `(0, 10, 0, 500) → None` (gap, not clamp); `(0, 0, 0, 500) → None`.
4. `scroll_top_to_reveal`: index past the window increases `scroll_top`; index in window unchanged; index 0 at top stays 0.
5. Host `include_str!("../ui/src/queue_window.rs")` matches the host module’s `visible_range` / new helpers (twin-drift guard). Path is from `crates/dedupe-chrome/src/`.

UI (`chrome-ui` wasm): queue page still renders; **no** `process-runner` in the ui crate. No page-snapshot harness exists (`path_id.rs` / `review_window.rs` tests only) — **header-outside is HITL** recorded in `review.md` (devtools: `.queue-header` is not a descendant of `.queue-window`).

Existing 0110–0116 chrome tests still pass.

---

## 4. Out of scope (do NOT do here)

- Three-pane coding / stale fetch (**0112** / **0118**).
- Produce wizard Finalize / privilege-log / cancelled-as-success (**0113** / **0119**).
- zpdf overlay / Burn counts (**0114** / **0120**).
- Image OPT QC (**0121**).
- Process extract-all Busy / orphan job snapshot (**0122**).
- Catalog write-lock (already read-first).
- Changing `visible_range` math, `PAGE_LIMIT`, `ROW_HEIGHT`, `OVERSCAN`.
- PageUp / PageDown keyboard paging.
- `use_infinite_scroll` as corpus strategy. Saved-search editor. Lead/QC default. Schema v42. BCC-default. Gutting `dedupe-desk`.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0111 Completed; `queue.rs` + `queue_window` still as §2.2.
- *Verified to date:* three Bugbot sites still live on HEAD `3bde470`; catalog path already read-first; schema v41.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Header sibling breaks extras columns | One `.queue-grid.extras` parent for header + rows |
| Windows scrollbar shifts 1fr columns | `overflow-y: scroll` + `scrollbar-gutter: stable` on header **and** viewport |
| Offset clamp Effect loop | Dedicated clamp Effect; write only when `new != old`; never in render |
| Empty-page fetch flashes corpus away | Keep last good `page` until next OK |
| `scroll_top` signal without DOM | Set `#queue.scrollTop` only when helper differs |
| Chip/keyword reset leaves stale scroll | `reset_queue_navigation()` on every offset-0 site |
| Twin helper drift | Host `include_str!("../ui/src/queue_window.rs")` parity test |
| Sticky-inside-scroller reintroduces 32px math | Header **above** the scroller, not sticky inside it |
| Stealing Process/Produce Bugbot | 0119 + 0122; §2.8 |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Header:** `.queue-header` is not inside the translated `.queue-window`. Spacer is rows-only. Header/viewport share `scrollbar-gutter: stable` + `overflow-y: scroll`. `visible_range` span still `≤ viewport_rows + 2*overscan` on the **row** list.
- [ ] **DoD-2 — Vacant honesty:** body `"0 in queue"` only when `total == 0`. Clamp Effect write-only-when-changed; last good page kept while refetching. Gap = banner + last rows, not vacant corpus. `current_idx` clamped after shrink.
- [ ] **DoD-3 — Keyboard:** arrowing through a fetched page keeps `.queue-row[aria-selected]` mounted; DOM `scrollTop` written only when `scroll_top_to_reveal` changes. Enter opens the on-screen current row. Shift+ArrowDown still opens current (not selection-extend). `reset_queue_navigation()` on chips/keyword/family/pager.
- [ ] **DoD-4 — Tests:** host helpers in §3.4 pass (including `include_str!("../ui/src/queue_window.rs")` twin parity); existing `visible_range_*` pass; `cargo test -p dedupe-chrome`; chrome-ui wasm still builds. No `unwrap`/`expect` in new production chrome/ui. No schema bump.
- [ ] **DoD-5 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0117-queue-virtualization` closed; ledger committed (`BUGFIX` or `FEATURE`). **0118–0122** stay Proposed unless separately implemented.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test -p dedupe-chrome
# chrome-ui job equivalent (re-verify workflow file at execute)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Do **not** `git add` operator PSTs or `output/`.

---

## 9. Deferred absorb / decline

| ID | Disposition |
|---|---|
| **D-0117-queue-virtualization** | **Absorb — this track.** |
| **D-0118-review-window-async** | Remain (**0118**). |
| **D-0119-produce-checklist-residuals** | Remain (**0119**). Sketch + PR **#123** cancelled-produce (not this track). |
| **D-0120-pdf-raster-ui** | Remain (**0120**). |
| **D-0121-image-opt-qc** | Remain (**0121**). |
| **D-0122-process-fold-residuals** | **Minted** from PR #123 Process Bugbot. Not this track. |
| **D-0026-01** | Decline (Desk windowed-list residual). |
| **D-0020-01** | Decline (operator GUI smoke). |
| **D-0110-deny-unic** | Remain (upstream unic). |
| **D-0062-codesign** | Remain. |
| PR #113 catalog write-lock | **Decline — already fixed** in 0112. |
| agy-O1 PageUp/PageDown | **Decline** — keyboard scope. |
| agy-m2 visible_range rewrite | **Decline** — already floor/ceil. |
| opencode-O1 / O2 pin/doctor | **Decline** — cosmetic. |
| BCC-default | Never. |

---

## 10. Unblocks

Counsel can page and keyboard the first-pass queue without a vacant lie or a ghost selection. **0118–0122** stay independent.
