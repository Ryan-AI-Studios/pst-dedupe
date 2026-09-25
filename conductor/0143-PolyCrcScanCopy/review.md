# 0143 — PolyCrcScanCopy — review

**Status:** Completed  
**Date:** 2026-09-25  
**PR:** [#162](https://github.com/Ryan-AI-Studios/pst-dedupe/pull/162)  
**Squash:** `1617720`

## DoD

| Item | Result |
|---|---|
| DoD-1 JSON copy | PASS — aspose `scan --json` / `dups --json` carry `poly_crc_note`; no root boolean |
| DoD-2 stderr | PASS — one `note:` after `run_scan`; `--crc-log-limit 0` still prints it |
| DoD-3 preflight | PASS — aspose stays `ok`; dual-rate / `compute_preflight` unchanged |
| DoD-4 omit when clean | PASS — key omitted at 0 sources; pre-0143 JSON deserializes |
| DoD-5 human + surfaces | PASS — `poly_sources=` / `poly_crc:`; keep-set/unique-pst/unique-eml wired |
| DoD-6 recorded | PASS — this file; registry Completed |

HITL INC* `scan --json`: **skipped** (spec optional).

## Gates

- `cargo fmt --all --check` PASS
- `cargo clippy --workspace --all-targets -- -D warnings` PASS
- `cargo test --workspace` PASS
- `ledgerful verify` PASS
- Ledger FEATURE `06bf42d6-ea74-472f-8b4d-e40b588d8526`

## Reviews

- Internal: PASS (DoD-6 process-open)
- Codex (`gpt-5.6-luna`): PASS (no P0–P3); DoD-6 process-open by instruction
