# 0104 — Attachment TC Multipage — Review

**Status:** Completed (engineering); publish in progress at write time  
**Branch:** `track/0104-attachment-tc-multipage`  
**Ledger:** `faab1343-e32e-4d07-87db-333fe9ed5c0f` (FEATURE `crates/pst-writer`)  
**PR / merge SHA:** filled after squash-merge

## DoD matrix

| DoD | Result | Evidence |
|---|---|---|
| **DoD-1 — Strategy A emit** | **PASS** | `build_attachment_table_strategy_a` + `AttachmentTableBuilt`; deleted `build_attachment_table_tc` + `heap_data_len`; matrix subnode; non-zero table `bid_sub`; own `sub_counter`/`table_subs`; trailing matrix `push` + `add_subnode_leaf`; empty messages omit `0x671`; store template unchanged |
| **DoD-2 — Tests** | **PASS** | `attachment_table_subnode` + `load_from_table_bids`; 1-row; 200×`attach_filename_test_{i:04}.txt` heap `>8176`; 328-row leaf-edge `get_row_string` 326/327; 1025-char cell NID SLBLOCK `len>=2` sorted; zero-attach omit; MessageSize inequality + `message_size_counts_attachment_table_matrix_bytes` (≥8201 residual) |
| **DoD-3 — Docs** | **PASS** | `unique-pst-export.md`, `pst-writer-fidelity-v1.md`, `CHANGELOG.md`; `D-0093-attachment-tc-page` **closed / 0104**; `D-0100-hn-bitmap-hdr` remains open (TC-heap wording) |
| **DoD-4 — Recorded** | **PASS** | This file; registry Completed on merge; ledger tx |

## Gates

| Gate | Result |
|---|---|
| Internal review (effort 1 general) | Round 1: 2 open (suggestion + nit) → fixed → re-review **0 open** |
| Codex `gpt-5.6-luna` high r1 (`review.codex.md`) | **FAIL** — P1 DoD-4 process (expected mid-flight); P2 MessageSize test strength |
| Codex P2 fix | New `message_size_counts_attachment_table_matrix_bytes` |
| Codex r2 (fresh) | **PASS** — no open P0–P2 product findings; DoD-4 noted as publish step |
| `cargo test -p pst-writer` fidelity attachment/MessageSize filters | ok |
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok (pre-P2 full workspace; post-P2 `-p pst-writer`) |
| `cargo test --workspace` | ok (pre-P2; post-P2 MessageSize + attachment_tc_ re-verified) |
| `ledgerful verify` | Verification passed; 0 pending / 0 unaudited drift at pre-publish checkpoints |

## Scope / residuals

- **Closed:** `D-0093-attachment-tc-page`
- **Unchanged residual:** `D-0100-hn-bitmap-hdr` (fail-closed for recipient + attachment TC heaps)
- No BCC-default change; no frontend steal of 0105+
- Series P **0099–0104** complete

## Notes

Locked fix only: reuse 0100 Strategy A on per-message `0x671`. Optional Outlook open of a large-attach unique-PST remains evidence-only, not CI.
