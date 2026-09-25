# 0142 — DupsJsonTotals — review

**Status:** Completed  
**Date:** 2026-09-25  
**PR:** [#160](https://github.com/Ryan-AI-Studios/pst-dedupe/pull/160)  
**Squash:** `a4dae1e`

## DoD

| Item | Result |
|---|---|
| DoD-1 Additive totals | PASS — `duplicates_total` == `summary.duplicates`; array still capped |
| DoD-2 Cap retained | PASS — `duplicates` remains `Vec<DupRow>`; default `--limit` 50 |
| DoD-3 Boundaries | PASS — `--limit 0` → `duplicates_limit` JSON `null`; zero-dup aspose; `limit >= total` truncated false |
| DoD-4 `scan --dups` | PASS — shared `DupsJsonPayload`; without `--dups` stays `duplicates: null` |
| DoD-5 Failure + human | PASS — fail envelope keys; `Duplicates (N of M shown):` |
| DoD-6 Recorded | PASS — this file; registry Completed |

HITL INC* `dups --json --limit 25`: **skipped** (spec optional).

## Gates

- `cargo fmt --all --check` PASS
- `cargo clippy --workspace --all-targets -- -D warnings` PASS
- `cargo test --workspace` PASS
- `ledgerful verify` PASS
- Ledger FEATURE `be647bcf-dc02-4aa3-8a7f-481fbbfd3aea`

## Reviews

- Internal: PASS (DoD-6 process-open)
- Codex (`gpt-5.6-luna`): PASS (no P0–P3); DoD-6 process-open by instruction
