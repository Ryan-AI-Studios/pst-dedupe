# 0100 — RecipientTcMultipage — Review

- **Track:** 0100-RecipientTcMultipage
- **PR:** [#90](https://github.com/Ryan-AI-Studios/pst-dedupe/pull/90)
- **Merge SHA:** `ab1c7b0a31d80a967a0d367234794ec233c87587`
- **Date:** 2026-08-27
- **Verdict:** Engineering DoD met. Codex luna r2 **PASS WITH DEFERRED P3**.

## Scope

Strategy A unique-PST recipient tables: every included To/Cc row (BCC still 0082 opt-in). Row matrix as a subnode packed with MS-PST §2.3.4.4 RowsPerBlock; multi-page HN on the recipient-table node only. Shared `TableContext::load_with_resolver` so four reader sites stop concatenating sibling SLENTRYs.

Out of scope (unchanged): attach-table writer Strategy A, HNBITMAPHDR, BCC default, message-PC 3580, 0101 depth, 0102 oracle attest (placeholder only).

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Rows | **PASS** | 140-row include-bcc fixture; 0 truncate counters/events; Display* full; continuation-page names round-trip |
| DoD-2 BCC | **PASS** | Default omit drops BCC rows; `--include-bcc-recipients` writes them |
| DoD-3 Layout | **PASS** | Non-empty `hnidRows` NID + `bid_sub`; empty `hnidRows=0` / `hidRowIndex=0` / `bid_sub=0`; 160-row span; `write_row_matrix_tree` unit test (width 100 dead space) |
| DoD-4 Reader | **PASS** | `load_tc`, `list_recipients`, embedded attach + recip tables use `load_from_table_bids`; RowsPerBlock; `get_row_string` HID/NID; cell collect string/binary only |
| DoD-5 Fail closed | **PASS** | No production Strategy B cap; typed `WriterError`; injected QC event kept |
| DoD-6 Docs | **PASS** | `docs/unique-pst-export.md`, `docs/pst-writer-fidelity-v1.md`; `D-0093-recipient-tc-multipage` closed in this governance commit |
| DoD-7 Recorded | **PASS** | This file; PR #90; SHA `ab1c7b0`; HITL INC* optional |

Live RowsPerBlock: `Floor(8176/56) = 146` (plan-time 145 was off-by-one).

## Reviewer rounds

| Round | Reviewer | Verdict |
|---|---|---|
| Implement | [Dedupe Implementor](c6dd5c49-e499-4948-96f2-1d46d87a5aa4) | Strategy A committed `a24d72c` |
| Internal | Orchestrator | Easy P3: integer cells not collected as HNIDs (`b82e8d2`) |
| Codex r1 | gpt-5.6-luna | **FAIL** — P1 governance (process, later); P2 weak packing tests; P2 145 vs 146 |
| Fix | Orchestrator | `2cf904b` packing/name/`hidRowIndex` tests + spec arithmetic |
| Codex r2 | gpt-5.6-luna | **PASS WITH DEFERRED P3** (`review.codex.r2.md`) |

P1 “governance not recorded” was classified **out of scope** for the engineering audit (Phase 8 after squash-merge).

## Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass (pre-commit / `ledgerful verify` / CI) |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass |
| `ledgerful verify` | pass |
| CI fmt / clippy / test / audit / deny / verify-parity | pass on PR #90 |
| Bugbot | skipping / not required |

## Deferred

| ID | Disposition |
|---|---|
| `D-0093-recipient-tc-multipage` | **Closed** on this track |
| `D-0100-hn-bitmap-hdr` | Residual — fail closed; pages 8/136/264 unimplemented |
| `D-0093-attachment-tc-page` | Residual — attach-table writer still single-page |
| `D-0094-inc-resmoke` | Optional operator INC* `output/inc0102784-post-0100/` (HITL, not CI) |

## HITL remaining

Operator unique-pst smoke on INC0102784 is optional. Outlook open of the synthetic 140-row PST is enough if skipped.
