# 0140 — ScanStderrCadence — review

**Status:** Completed  
**Date:** 2026-09-25  
**PR:** [#156](https://github.com/Ryan-AI-Studios/pst-dedupe/pull/156)  
**Squash:** `0839c38`

## DoD

| Item | Result |
|---|---|
| DoD-1 Default + `-v` volume | PASS — helper 40-tick; aspose CLI default 0, `-v` cadence, `-vv` one per folder (`folder=` without `msg_i=`, ANSI-stripped) |
| DoD-2 `-vv` documented | PASS — global `--help` scan-family; if/else info/debug |
| DoD-3 Probe cadence | PASS — helper 1/500 when first_n > 0 |
| DoD-4 `--crc-log-limit 0` | PASS — aspose deep-attach JSON enabled, zero per-attempt lines; unique-pst summary retained; five clap surfaces |
| DoD-5 Isolation | PASS — `--json` parses; no `--progress-file`; `stage=` unchanged |
| DoD-6 Recorded | PASS — this file; registry Completed |

HITL INC* `scan -v`: **skipped** (spec optional).

## Gates

- `cargo fmt --all --check` PASS
- `cargo clippy --workspace --all-targets -- -D warnings` PASS
- `cargo test --workspace` PASS
- `ledgerful verify` PASS
- Ledger FEATURE `d3ceac13-ad1c-4b82-a998-ff14bfb06f66`

## Reviews

- Internal: PASS WITH DEFERRED P3 (unique-pst CLI spawn not added; helper shared)
- Codex (default model): FAIL on dirty tree DoD-6 (process-timing) + P2 test classifier; P2 fixed (ANSI-strip `folder=` without `msg_i=`)

## Publish

- PR **#156** / `0839c38`
