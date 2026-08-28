# 0103 — Recipient TC SLBLOCK NID Order — Review

**Status:** Completed  
**Branch:** `track/0103-recipient-tc-slblock-nid-order` (deleted after merge)  
**Product commit:** `2664c3d` — `track(0103): recipient TC SLBLOCK NID order`  
**PR:** [#96](https://github.com/Ryan-AI-Studios/pst-dedupe/pull/96)  
**Merge SHA:** `f66ae9b9ad52fcca0a8b75248cfe64f7f7d295c6`  
**Ledger:** `43a45212-a41b-4cfc-8a21-242ccbbdffc9` (BUGFIX `crates/pst-writer`; promoted by post-commit hook)

## DoD matrix

| DoD | Result | Evidence |
|---|---|---|
| **DoD-1 — Emit order** | **PASS** | `insert(0)` gone; matrix `push` after row loop; `add_subnode_leaf` sorts NID ascending; duplicate → `WriterError::Layout` with hex NID |
| **DoD-2 — Tests** | **PASS** | Unit ascending + duplicate; fidelity long-display SLBLOCK `len==3`; dual-string `len==4` via `list_subnode_entries` |
| **DoD-3 — Docs** | **PASS** | `unique-pst-export.md`, `pst-writer-fidelity-v1.md`, `CHANGELOG.md`; `D-0100-slblock-nid-order` **closed / 0103** |
| **DoD-4 — Recorded** | **PASS** | This file; registry Completed on merge; ledger tx committed |

## Gates

| Gate | Result |
|---|---|
| Internal review (effort 1 general) | **0 open issues** |
| Codex `gpt-5.6-luna` high (`review.codex.md`) | **PASS** — no P0–P3 |
| `cargo test -p pst-writer add_subnode_leaf` | 2 passed |
| `cargo test -p pst-writer recipient_tc` | 6 passed |
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok |
| `cargo test --workspace` | ok |
| `ledgerful verify` | Verification passed; 0 pending / 0 unaudited drift |
| Required CI on PR #96 | **green** — fmt, clippy, test, audit, deny, verify-parity (Bugbot pass, non-blocking) |

## Scope / residuals

- **Closed:** `D-0100-slblock-nid-order`
- **Unchanged:** `D-0100-hn-bitmap-hdr`, `D-0093-attachment-tc-page`, `D-0094-inc-resmoke` (operator)
- No BCC-default change; no frontend steal of 0104 (0105+)
- Series P **0099–0103** complete unless a new residual is found

## Notes

Locked fix only: trailing matrix push + emit-sort. Message-level SLBLOCK reorder by NID is intended. Optional Outlook open of a long-display unique-PST remains evidence-only, not CI.
