# 0101-EmbeddedDepthFlag — completion review

- **Track:** 0101-EmbeddedDepthFlag
- **Branch:** `track/0101-embedded-depth-flag`
- **Status at implement:** In Progress (registry Completed after squash-merge)
- **HITL:** **skipped** — no operator INC* `--max-embedded-depth 8` smoke this pass (`D-0094-inc-resmoke` remains residual)
- **PR / SHA:** filled after publish

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| **DoD-1 Wire** | **PASS** | `--max-embedded-depth` on unique-pst; default 3; clap rejects &lt;1 and &gt;8 (`parse_max_embedded_depth_arg`). Single `nested_extract_depth = args.max_embedded_depth.clamp(1, 8)` to `materialize_nested_for_winner`, `WritePstOpts`, and named-prop `scan_messages_with_depth`. GUI `unique_wizard.rs` compile default 3, no slider. Identity `MAX_EMBEDDED_IDENTITY_DEPTH = 3` unchanged. |
| **DoD-2 Synthetic** | **PASS** | `unique_pst_depth.rs` (does **not** use `unique_pst.rs` `--no-attachments` helper): depth-4 fails at 3 / succeeds at 4; 8-deep fails at 7 / succeeds at 8; clap 0/9/abc require `"1 to 8"`; library 0→1 and 9→8. Writer `embedded_depth_chain_of_nine_halts_at_eight`. No client PSTs. |
| **DoD-3 Honesty** | **PASS** | `export.max_embedded_depth` always serialized (`unique_export_report_v1` id unchanged). Asserted on default 3, flag 4, library clamps, and cancel (requested 4→4 and 9→8). Remaining over-depth still `ATTACH_DEPTH_LIMIT`. Docs + runbook one-liner. |
| **DoD-4 Recorded** | **pending publish** | This file; ledger FEATURE on `crates/pst-dedup-cli`; `D-0067-embedded-depth` narrowed (CLI shipped, **not closed**); HITL skipped as above. |

## Reviewer rounds

| Round | Verdict | Notes |
|---|---|---|
| Internal | FAIL (easy P3) | Clap reject accepted generic `"error"`; cancel echo only asserted at hardcoded 3 |
| Internal fix | — | Require `"1 to 8"`; `cancel_summary_echoes_effective_depth` on synthetic PST |
| Internal re-review | **PASS** | Prior P3s fixed; no new &gt;low |
| Codex (`gpt-5.6-luna` high, read-only) | **PASS** | No P0–P3. `review.codex.md` |

## Local gates (observed)

| Command | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test -p pst-dedup-cli --test unique_pst_depth` | 6 passed |
| `cargo test -p pst-writer embedded_depth` | PASS |
| `cargo test --workspace` | PASS |
| `ledgerful verify` | PASS (fmt + clippy + workspace test) |

## Deferred

- **D-0067-embedded-depth:** unique-pst CLI 1–8 (default 3) shipped. Residual: unique-eml `message/rfc822`, matter/Relativity children, 32 MiB per-nest, hard cap 8. **Do not close.**
- **D-0094-inc-resmoke:** HITL skipped. Operator may re-export INC* at `--max-embedded-depth 8` locally.

No residual lows from this track parked beyond those rows.

## Locks held

Default depth **3**; clap **rejects** outside 1–8; identity hash depth **3**; BCC default **off**; unique-eml OOS; 0102/0103 not implemented here.
