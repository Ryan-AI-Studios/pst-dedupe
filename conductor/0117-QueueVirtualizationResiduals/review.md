# 0117 — QueueVirtualizationResiduals — Review

## Scope

First-pass queue honesty after PR **#113** Bugbot: header sibling above `#queue`, empty page ≠ empty corpus, arrows keep the current row in the DOM window. Schema stays **41**. **0118–0122** product code not implemented (0122 placeholder spec/plan minted only).

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Header | PASS (static) | `.queue-header` is a sibling above `#queue.queue-viewport`; spacer is `fetched_len * ROW_HEIGHT`; CSS `overflow-y: scroll; scrollbar-gutter: stable` on header and viewport. HITL: release EXE, confirm `.queue-header` is not a descendant of `.queue-window`; Windows classic scrollbar column alignment. |
| DoD-2 Vacant honesty | PASS | Body `"0 in queue"` only when `total == 0`. Dedicated clamp Effect via `clamp_offset_for_fetch_meta` (write only when changed; ignores stale meta offset). Gap: banner + last-good page; Next stays enabled (`next_page_disabled`). `current_idx` clamped after shrink. |
| DoD-3 Keyboard | PASS (static) | `scroll_top_to_reveal` + `#queue.scrollTop` only when the helper differs. `#queue` is outside the `scroll_top` inner closure. Shift+ArrowDown remains open-current. `reset_queue_navigation()` on chips/keyword/family/pager/matter params. |
| DoD-4 Tests | PASS | `cargo test -p dedupe-chrome` — 108 passed. Host helpers + twin `include_str!("../ui/src/queue_window.rs")`. `cargo check --target wasm32-unknown-unknown` in `crates/dedupe-chrome/ui`. No `unwrap`/`expect` in new production queue code. Schema 41. |
| DoD-5 Recorded | PASS | This file; registry **Completed**; `D-0117-queue-virtualization` closed. Ledger BUGFIX `52da3f04` committed on the product squash. |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo clippy -p dedupe-chrome --all-targets -- -D warnings` | pass |
| `cargo test -p dedupe-chrome` | 108 passed |
| `cargo test --workspace` | pass (independent; `ledgerful verify` workspace-test step timed out at 300s once, then passed on pre-push in 260s) |
| chrome-ui wasm check | pass |
| CI (PR **#125**) | fmt, clippy, test, audit, deny, chrome-ui, verify-parity **green**. Bugbot NEUTRAL (skipping). |
| Codex r3 | **PASS**, no findings |

## Reviewer rounds

1. Internal r1: FAIL — P0 `#queue` remounted inside `scroll_top` closure. Fixed (stable `#queue`, inner rows-only).
2. Internal r1 P2: Next disable + shrink `scroll_top`. Fixed.
3. Codex r1: FAIL — P1 gap pager both-disabled; P1 clamp helper in fetch Effect. Fixed (`next_page_disabled`; fetch classifies `offset < total`).
4. Codex r2: FAIL — P1 stale `last_fetch_meta` clamped Next back to 0. Fixed (`clamp_offset_for_fetch_meta` requires matching offsets).
5. Codex r3: **PASS**. Fresh pass; no open >low.

## HITL (owner)

Release chrome EXE, synthetic Unreviewed page: arrows, last-page bulk tag, header-outside in devtools, Windows classic scrollbar alignment. Codesign is **D-0062-codesign**. INC* unique-pst is not a gate.

## Publish

- Branch: `track/0117-queue-virtualization`
- PR: **#125**
- Merge SHA: `199975cba0f33d4f578230b83303e4e5bddae82d`
