# 0124 — ReviewQueueChrome — Review

## Scope

First-pass queue cells ellipsis (`minmax(0,…)` + overflow/nowrap) so long X500 From/Subject never paint on Fam/Resp/PRIV. 244px rail (Unreviewed / Privileged / Responsive + saved searches; Needs decision / Redaction QC / Consistency inert). Toolbar title is the active queue + count. `QueueChromeCtx` on the **queue** route only fills the 0123 TopBar right slot (`#queue-goto`) and StatusBar left (SQL-page `Rows a–b of N`). **Select page ({len})** plus existing Tag… / privilege preview. Schema stays **41**. **0117** `queue_window.rs` math untouched. **0125** / **0126** not implemented.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 No collision | PASS | Default + extras tracks `minmax(0, …)`. Cell selector `overflow:hidden; text-overflow:ellipsis; white-space:nowrap`. From/Subject/Custodian `title` = display; Control# `title` = item id. `ROW_HEIGHT` 32. CSS/helper tests. |
| DoD-2 Rail + toolbar | PASS | `.queue-layout { 244px 1fr }`. Unreviewed → `preset_uncoded_json`. Saved searches in the rail. `h1` + `aria-label` = `{name} {total} docs`. Button **Save search**. Inert later rows count 0 + “no filter yet”. |
| DoD-3 Go-to + range + bulk | PASS | `WrapReview` provides `QueueChromeCtx`; `wrap_review_window` does not. `#queue-goto`; Ctrl+K after `#matter-search`. Integer miss `Control# N not found in current page (Rows a–b)`. StatusBar `QueueRange::status_label`. Select page uses `p.rows` ids. Mount + timeout 0 + resize measure `#queue.client_height` (`viewport_h` inits 0). Enter/click still `review_doc_href`. |
| DoD-4 Hygiene | PASS | No new production `unwrap`/`expect`. Schema 41. Plex + `#1b3049`. 0117 twin + 0122 Busy + 0123 shell locks green. UI 38 tests; `cargo test -p dedupe-chrome`; workspace + clippy `-D warnings`. chrome-ui CI trunk build green. |
| DoD-5 Recorded | PASS | This file; registry **Completed**; `D-0124-review-queue-chrome` closed. Ledger FEATURE committed on the product squash. Residual low: first paint mounts ~8 overscan rows until measure (HITL). |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` | pass (38) |
| `cargo test -p dedupe-chrome` | pass (117, including 12 `queue_window` twins) |
| `cargo test --workspace` | pass |
| `ledgerful verify` | pass |
| CI (PR **#141**) | fmt, clippy, test, audit, deny, chrome-ui, verify-parity **green**. Bugbot NEUTRAL (does not block). |
| Final cross-model gate | **CLEAN**, no open >low (`review.codex-final.md`) |

## Reviewer rounds

1. Internal: DoD-1…4 Met. Lows: CSS lock not selector-tight (fixed); `viewport_h` init 640 (changed to 0). **PASS** (no >low).
2. Codex r1: **PASS WITH DEFERRED P3** (no P0–P2). Easy P3 CSS lock + first-frame 640.
3. Final gate (fresh): **CLEAN**. Residual P3 first-frame overscan-8 until measure (HITL).

## HITL (owner)

Release chrome EXE on a synthetic matter with a long Exchange X500 `from_addr` and a queue **>500** rows: From/Subject must **ellipsis**, never paint on Fam/Resp/PRIV; extras grid also clips. After Next, StatusBar left shows a truthful `Rows 501–… of N`. Enter / row click still opens **0112**. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.

## Publish

- Branch: `track/0124-review-queue-chrome`
- PR: **#141**
- Merge SHA: `ff8b0eac67bf71097eeca93bf074b83424035f4e`
