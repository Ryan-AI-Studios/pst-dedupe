# Track Completion Review — 0074-DeepAttachPreflightFidelity

## Verdict: PASS WITH DEFERRED P3

**Date:** 2026-07-28  
**Branch:** `track/0074-deep-attach-preflight`  
**Cross-model final gate:** Codex `gpt-5.6-luna` high — `review.codex.final5.md` (**PASS WITH DEFERRED P3**)

## Scope

Budgeted deep attachment stream preflight (Series L P0):

- Graded L1–L3 probe (default L2 head), hard budgets, per-attach timeout (thread + `recv_timeout`)
- Opt-in `--deep-attach-preflight` on `scan` and `unique-pst` (default **off**)
- Shared `ATTACH_*` reason strings with 0073
- Preflight `attach_probe` + `max_attach_fail_rate` → `re_export_recommended`
- Winner/group probe before resolve; peer-cap marks unprobed peers degraded
- Level-aware cache (level + mtime + source file size) → materializer `stream_available`
- Strict mode: probe fails → skip + full tally/result rebuild
- Cancel: non-fail; incomplete coverage honesty
- Docs: honesty, ScanPST-on-copy / re-export, 0073 residual ledger

## Review rounds

| Round | Reviewer | Verdict |
|---|---|---|
| Internal DoD + correctness | explore subagents | FAIL → fixed (cancel, tallies, strict, peer cap, …) |
| Internal re-review | explore | PASS WITH DEFERRED P3 |
| Codex luna #1 | `review.codex.md` | FAIL |
| Fix pass | implement subagent | P1/P2 closed |
| Codex luna #2–4 | final, final2, final3, final4 | FAIL (reconciliation, budget, peer-cap winner) |
| Fix passes | implement + orchestrator | closed remaining P1/P2 |
| **Codex luna #5 (final gate)** | `review.codex.final5.md` | **PASS WITH DEFERRED P3** |

## DoD matrix (final)

| DoD | Status |
|---|---|
| 1 Probe engine L1–L3 + budgets | Met |
| 2 Winner path unique-pst | Met |
| 3 Scan optional flag | Met |
| 4 Shared 0073 reasons | Met |
| 5 Preflight attach rates | Met |
| 6 Keep-set fidelity + peer cap | Met |
| 7 stream_available honesty | Met (cache bridge) |
| 8 Safety (no fat Vec, cancel, probe LRU) | Met; mat-lru residual |
| 9 parents_only skip | Met |
| 10 Honesty docs | Met |
| 11 Per-attach timeout | Met; timeout-join residual |
| 12 Peer probe cap | Met |
| 13 Cache level/size/mtime | Met; cross-process share residual |
| 14 Tests | Met unit matrix; E2E fixture residual |
| 15 Docs | Met |
| 16 Recorded | This file + registry + D-0074-* |

## Deferred residuals (D-0074-*)

| ID | Item |
|---|---|
| D-0074-gui | Wizard checkbox for deep-attach (CLI works; defaults off) |
| D-0074-mat-lru | Bound materializer/export sticky PST HashMap to max_open_psts |
| D-0074-cache-share | Cross-process / scan→unique disk cache |
| D-0074-crc-fixture | Synthetic corrupt-attach PST E2E CRC fixture |
| D-0074-timeout-join | Join/cancel timed-out probe worker (abandoned background) |
| D-0074-e2e-fixture | Full production-path §3.11 fixture matrix beyond unit coverage |

## Gates (orchestrator observed)

- `cargo fmt --all` OK  
- `cargo test -p dedup-engine --lib` — 94 passed  
- `cargo test -p pst-dedup-cli --lib` — 82+ passed  
- `cargo clippy -p dedup-engine -p pst-dedup-cli --all-targets -- -D warnings` OK  
- `cargo check -p pst-dedup-gui` OK  

Full workspace gate run before PR merge.

## Product locks recorded

- Deep probe **opt-in** (scan + unique-pst default off)  
- Default level **L2 head**  
- Phase 1b budgeted probe; materializer does not re-probe under separate unlimited budget; uses `ProbeResultCache` for `stream_available`  
- Peer cap: unprobed remaining peers marked `ATTACH_PEER_PROBE_CAP` + degraded  

## Completion decision

Engineering DoD met. **Completed** after registry + deferred ledger + PR CI green + squash merge.
