# 0139 — SourceRankDiscoverability — review

**Status:** engineering DoD-1–5 complete; HITL optional skipped.  
**Branch:** `track/0139-source-rank-discoverability`  
**Ledger:** `8437677d-6fb7-475e-9eb9-1f080166b1b8` FEATURE  
**HEAD at implement:** `791d230` + this branch

## DoD

| Item | Result |
|---|---|
| DoD-1 Help | PASS — keep-set / unique-pst / unique-eml `--source-rank` names lexicographic path-sort and `-2.pst` |
| DoD-2 JSON | PASS — `input_path_sort_order` on keep-set, unique-pst (`inputs` kept equal), unique-eml (both construction sites); on-disk keep-set summary asserted |
| DoD-3 Note | PASS — helper after `sort_input_paths`; `--source-rank` suppresses; `--prefer-path-contains` does not; `--json` stdout parseable; also-eml inner does not emit |
| DoD-4 Oracle | PASS — root-remove; not allowlisted; parent vs HEAD unit |
| DoD-5 Tests | PASS — helper unit, keep-set CLI, help, oracle; existing `source_rank_*` green |
| DoD-6 Recorded | PASS — this file, CHANGELOG, export.md, ledger FEATURE, D-0139 closed |

## Gates

- `cargo fmt --all --check` PASS
- `cargo clippy --workspace --all-targets -- -D warnings` PASS
- `cargo test --workspace` PASS
- `ledgerful verify` PASS

## Reviews

- Internal: PASS WITH DEFERRED P3 (optional unique-pst/unique-eml CLI tests; also-eml count==1 untested — pack inner has no helper)
- Codex: FAIL on DoD-6 process-timing only; no product P0–P2. Disposition: validated as closeout remaining at audit time.

## HITL

Owner INC* two-file `keep-set --json` **skipped** (spec optional). Never committed INC* PSTs.

## Also-eml

`unique-pst --also-eml` calls `write_eml_pack_from_keep_set`, not `run_unique_eml`. Helper is not in the pack inner. Two-file also-eml `count==1` not added (fixtures are single-input).

## PR

Filled after merge.
