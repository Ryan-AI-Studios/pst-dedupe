# 0118 — ReviewWindowAsyncResiduals — Review

## Scope

Three-pane window honesty after PR **#115** Bugbot: stale `review_document` / `review_document_body` must not paint the previous item; persist that stays on the same item must refresh `doc.codes` / `doc.notes`. Schema stays **41**. **0119–0122** product code not implemented.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Stale fetch | PASS | `fetch_is_current(id, gen)`; `doc_generation` / `body_generation` increment before `spawn_local`; catalog + document + body OK/Err gated; body also `b.pane == pane`. Raster `raster_generation` unchanged. Helper tests: match / id mismatch / gen mismatch. |
| DoD-2 Same-item save | PASS | Persist that does not navigate (`then_next && next_id is None`, or `then_next == false`) increments `doc_generation`, re-fetches `review_document`, overwrites `pending_*` from `codes_state`. Refresh fail: status only, keep `doc`. `persist_holds_save_for_refresh`: Save stays locked until refresh returns. Navigate: `go_item` only. |
| DoD-3 path_id | PASS | Single `#[test]` on `review_doc_href_encodes_filter_and_keyword`; `#[test]` on `stub_back_href_reencodes_decoded_windows_param`. chrome-ui: `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml`. |
| DoD-4 Hygiene | PASS | No new production `unwrap`/`expect`. Schema 41. Host `review_window_apply` sequence unchanged. Overlay/draw not rewritten. `cargo test -p dedupe-chrome` 108 passed; ui crate 11 passed; wasm check passed. |
| DoD-5 Recorded | PASS | This file; registry **Completed**; `D-0118-review-window-async` closed. Ledger BUGFIX `f28ca241` committed on the product squash. |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` | 11 passed |
| `cargo test -p dedupe-chrome` | 108 passed |
| `cargo check --target wasm32-unknown-unknown` (ui) | pass |
| `cargo test --workspace` | pass (independent ~406s; `ledgerful verify` step timed out at 300s once; pre-push passed in 273s after a local test-step timeout of 600s) |
| CI (PR **#127**) | fmt, clippy, test, audit, deny, chrome-ui, verify-parity **green**. Bugbot NEUTRAL (skipping). |
| Codex r2 | **PASS**, no findings |

## Reviewer rounds

1. Internal: DoD-1…4 wired; host apply / raster / schema untouched.
2. Codex r1: FAIL — P2 same-item refresh left `saving=false` so a follow-up save could diff stale `doc.codes`. Validated and fixed (`persist_holds_save_for_refresh`; Save disabled through refresh).
3. Codex r2: **PASS**. Fresh pass; no open >low.

## HITL (owner)

Release chrome EXE, synthetic 3-doc Unreviewed family: rapid Save & Next / `[` `]`, then Enter on the last item, un-check privilege, Save/Enter. Codesign is **D-0062-codesign**. INC* unique-pst is not a gate.

## Publish

- Branch: `track/0118-review-window-async`
- PR: **#127**
- Merge SHA: `74fd7975a928df6badb8ddcee248b9f3fb959182`
