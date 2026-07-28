# 0079 Baseline — Phase timings

## Machine

| Field | Value |
|---|---|
| Hostname | Desktop |
| OS | Microsoft Windows NT 10.0.26200.0 |
| Build | `track/0079-materialize-write-performance` debug `pst-dedup` |
| Date | 2026-07-28 |

## Honesty note on “before”

Phases 0–5 landed in one change set on this branch. A pure pre-optimization
instrumented run on this machine is therefore **not reconstructible**. What is
recorded here:

1. **Historical operator wall (pre-track):** INC unique-pst ~**275 s**, scan
   ~**3 s**, 3728 winners, 366 attach fails (from track evidence; source PSTs
   not in git).
2. **Fixture post-0079 numbers** (this machine, after Phases 0–5). These are
   the attributable phase split going forward; they prove instrumentation and
   the D1 invariant (`messages_materialized == unique`).
3. **Structural pre-D1 fact:** before Phase 1 every winner was materialized
   twice (`finalize_with_materialize` + `prepare_winner` re-materialize). After
   Phase 1, `messages_materialized == unique` on the fixture.

## Fixture: `fixtures/aspose_outlook.pst` (~3.2 MiB)

### Run A — `--no-attachments`

| Metric | Value |
|---|---|
| `unique` / written | 17 / 17 |
| `messages_materialized` | **17** (= unique) |
| `source_pst_opens` | 1 |
| `bytes_written_total` | 194000 |
| `prepared_bytes_peak` | 137815 |
| `hash_ms` | 17 |
| wall (process) | ~345 ms |
| `duration_ms` | 315 |

| Phase | ms |
|---|---|
| scan_ms | 18 |
| deep_attach_preflight_ms | 0 |
| resolve_ms | 0 |
| materialize_ms | 9 |
| prepare_ms | 0 |
| write_ms | 31 |
| report_ms | 1 |
| verify_ms | 4 |
| quarantine_ms | 0 |
| unaccounted_ms | 252 |
| total_ms | 315 |

### Run B — attachments on (default family)

| Metric | Value |
|---|---|
| `unique` / mat | 17 / **17** |
| `source_pst_opens` | 1 |
| wall | ~324 ms |
| `duration_ms` | 298 |

| Phase | ms |
|---|---|
| scan_ms | 11 |
| materialize_ms | 12 |
| write_ms | 50 |
| report_ms | 1 |
| verify_ms | 3 |
| unaccounted_ms | 221 |
| total_ms | 298 |

`unaccounted_ms` is honest (path guards, report-dir prep, clap/setup, summary
JSON emit). Non-zero is expected on short fixture runs where setup dwarfs work.

## Cancel latency baseline

0078 cancel/quarantine suite remains the contract gate. Fixture-scale cancel
mid-write is covered by existing `export_exit_0078` tests (exit 130 +
`artifact_state`). No operator multi-GB cancel measurement available on this
machine (D-0079-operator-multigb residual).

## Complexity baseline (AMap, operation count)

Post Phase 2 unit test `amap_scan_steps_linear_in_block_count`: 200 vs 800
blocks; steps/block ratio does not grow superlinearly (asserted in CI).
