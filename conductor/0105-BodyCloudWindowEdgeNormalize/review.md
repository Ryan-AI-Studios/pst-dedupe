# 0105 — BodyCloudWindowEdgeNormalize — Review

**Status:** Completed (engineering + publish)  
**Branch:** `track/0105-body-cloud-window-edge-normalize`  
**Ledger tx:** `547f1fc7-2c03-42f7-99c7-4370fc92d6c2` (BUGFIX / crates/dedup-engine)

## DoD matrix

| DoD | Result | Evidence |
|---|---|---|
| DoD-1 Edge identity | **PASS** | `handle_window_edge_bare` normalizes with `strip_trailing_punct=true` before classify; empty/unclassified never inserted; `note_overlength` inserts classified `final_url` into `seen`; max-links check before that insert; unique unseen cuts still WINDOW |
| DoD-2 Tests | **PASS** | New `body_window_duplicate_cut_url_trailing_period_not_dropped` (`head+tail == url + "."`) and `body_window_overlength_then_edge_duplicate_not_window`; both fail on unpatched HEAD; keep-green set observed |
| DoD-3 Docs | **PASS** | `docs/unique-pst-export.md` honesty clause; CHANGELOG Unreleased; `D-0097-window-edge-normalize` **closed / 0105** |
| DoD-4 Recorded | **PASS** | This file; registry Completed; ledger commit on publish |

## Gates

| Gate | Result |
|---|---|
| `cargo test -p dedup-engine body_window` | 8 passed |
| `cargo test -p dedup-engine --lib body_cloud` | 43 passed |
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass (direct) |
| `ledgerful verify` | see publish note if alias timeout; direct verify.commands green |
| Codex `review.codex.md` | **PASS** (0 findings) |

## Internal review

- Round 1: 3 open (2 suggestion + 1 nit) — design-narrating comments
- Fix: constraint-only / fixture-fact comments
- Re-review: **0 open**

## Codex

- Primary: gpt-5.6-luna / high → **PASS**, no P0–P3
- File: `review.codex.md`

## Deferred

- Closed: **D-0097-window-edge-normalize**
- No new lows from this track
- Declined residuals unchanged (D-0088, D-0094, D-0100, D-0099, frontend **0106+**)

## Publish

| Item | Value |
|---|---|
| PR | [#100](https://github.com/Ryan-AI-Studios/pst-dedupe/pull/100) |
| Merge SHA | `dfd19bd` |
| CI note | First `test` run flaked on unrelated `process-runner::start_succeeds_exactly_one_job_row` (`wait_until_idle` 5s); rerun green. Required checks + verify-parity passed. |

## Notes

- Series Q **0105** closed. Frontend / Hermes Series O, if started, uses **0106+**.
- No HITL / INC* smoke required.
- No BCC / HNBITMAPHDR / attach-CRC scope creep.
