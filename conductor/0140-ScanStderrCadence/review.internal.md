# Track Completion Audit — 0140-ScanStderrCadence

**Reviewer:** internal  
**Date:** 2026-09-25  
**Verdict:** PASS WITH DEFERRED P3 (DoD-1–5; DoD-6 at finalize)

## Requirement and DoD Matrix

| DoD | Met | Evidence |
|---|---|---|
| 1 Default + `-v` volume | Met | Helper 40-tick unit; aspose CLI: default 0 `scan progress`, `-v` cadence (2 lines), `-vv` one per folder |
| 2 `-vv` documented | Met | Global `--help` names scan/dups/keep-set periodic vs per-folder; if/else info/debug |
| 3 Probe cadence | Met | `should_emit_probe_progress_line` 1/500 when first_n>0 |
| 4 `--crc-log-limit 0` | Met | Aspose `--deep-attach-preflight --crc-log-limit 0`: zero per-attempt lines, JSON `attach_probe.enabled=true`. unique-pst summary left. Help on five clap surfaces. `first_n` captured outside closure |
| 5 Isolation | Met | `--json` stdout parses; no `--progress-file`; `stage=` untouched |
| 6 Recorded | Open | Finalize after gates/PR |

## Findings

P0–P2: none.

P3: unique-pst per-attempt silence is compile-enforced via shared helper; no unique-pst `--deep-attach-preflight --crc-log-limit 0` CLI spawn (scan covers the helper + scan sink). Owner INC* HITL optional.

## Wiring

Folder loop per file: timer re-arm on emit. Probe `progress_cb` in `scan.rs` and `unique_pst_cmd.rs`. Getter `log_first_n` under `TEST_LOCK`.
