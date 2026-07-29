# 0079 Baseline — Phase timings + parent-vs-HEAD

## Machine

| Field | Value |
|---|---|
| Hostname / ComputerName | Desktop / DESKTOP |
| OS | Microsoft Windows NT 10.0.26200.0 |
| Parent binary | `9c8be49` worktree `C:\dev\Dedupe\.wt-0079-baseline` → `target\debug\pst-dedup.exe` |
| HEAD binary | `track/0079-materialize-write-performance` debug `target\debug\pst-dedup.exe` |
| Fixture | `fixtures/aspose_outlook.pst` (~3.2 MiB, 17 unique) |
| Date | 2026-07-28 |

## Parent vs HEAD (orchestrator gate)

Worktree: `git worktree add C:\dev\Dedupe\.wt-0079-baseline 9c8be49`
Build: `cargo build -p pst-dedup-cli` in worktree and at HEAD.
Oracle: current-branch `pst_dedup_cli::export_oracle::compare_export_packs` (allowlist strips additive 0079 counters so parent packs without them still compare).
Optional CI re-run: `PST_DEDUPE_BASELINE_BIN=<parent pst-dedup.exe> cargo test -p pst-dedup-cli --test unique_pst unique_pst_parent_baseline_oracle_when_env_set`.

Packs used for recorded compare (also under `output/0079-parent-vs-head/` — ephemeral):

| Mode | Parent report / out | HEAD report / out |
|---|---|---|
| `--no-attachments` | `report_parent_noatt` / `parent_noatt.pst` | `report_head_noatt` / `head_noatt.pst` |
| attachments on (`--allow-partial-fidelity`) | `report_parent_att` / `parent_att.pst` | `report_head_att` / `head_att.pst` |

### Wall / `duration_ms` (warm, debug, n=3 after warmup)

| Run | parent wall_ms | parent duration_ms | HEAD wall_ms | HEAD duration_ms | HEAD messages_materialized |
|---|---:|---:|---:|---:|---:|
| noatt r1 | 320 | 272 | 281 | 247 | **17** (= unique) |
| noatt r2 | 300 | 265 | 291 | 255 | **17** |
| noatt r3 | 288 | 253 | 283 | 249 | **17** |
| att r1 | 354 | 318 | 305 | 268 | **17** |
| att r2 | 346 | 299 | 309 | 273 | **17** |
| att r3 | 329 | 293 | 303 | 268 | **17** |

**Medians (duration_ms):** noatt parent **265** vs HEAD **249**; attachments-on parent **299** vs HEAD **268**.
Fixture residual is **sub-second** for both binaries. Parent summary has **no** `messages_materialized` / `phase_timings` / `source_pst_opens` (pre-0079); HEAD asserts `messages_materialized == unique` (17).

### Oracle result

- Parent vs HEAD `--no-attachments`: **equivalent** (`unique_pst_parent_baseline_oracle_when_env_set` green with `PST_DEDUPE_BASELINE_BIN`).
- Parent vs HEAD attachments-on: **equivalent** (same test).
- Per-winner `degraded_reasons` parent vs HEAD: **0 diffs** (manual keepset compare + oracle keepset path).
- Attachments-on product: `attachments_failed=4` (`ATTACH_EMBEDDED_UNPARSED`) both sides; `attachments_written=30`.

### HEAD structural counters (r3 noatt / att)

| Metric | noatt | att |
|---|---:|---:|
| unique | 17 | 17 |
| messages_materialized | 17 | 17 |
| source_pst_opens | 1 | 1 |
| bytes_written_total | 194000 | 775184 |
| prepared_bytes_peak | (from run) | 378241 |
| hash_ms | ~11 | ~30 |

## Historical operator wall (pre-track; multi-GB)

INC unique-pst ~**275 s**, scan ~**3 s**, 3728 winners, 366 attach fails (source PSTs **not** in git).
**No multi-GB parent-vs-HEAD measured on this machine** → residual `D-0079-operator-multigb`. Do **not** invent multi-GB numbers.

## HEAD phase split (instrumented; post Phases 0–5)

Example noatt (`duration_ms` 249–271 band):

| Phase | ms (representative) |
|---|---:|
| scan_ms | 18 |
| deep_attach_preflight_ms | 0 |
| resolve_ms | 0 |
| materialize_ms | 7 |
| prepare_ms | 0 |
| write_ms | 21 |
| report_ms | 4 |
| verify_ms | 2 |
| quarantine_ms | 0 |
| unaccounted_ms | ~219 |
| total_ms | ~271 |

`unaccounted_ms` is honest (path guards, report-dir prep, clap/setup, summary JSON emit). Non-zero is expected on short fixture runs.

### Why parent has no per-phase table (DoD-4 honesty)

Parent `9c8be49` has **only** total `duration_ms` — `PhaseTimings` did not exist.
Per-phase before/after on the *same* instrumentation is therefore **impossible**
without rewriting parent history. What this track records instead:

| Measurement | Parent | HEAD | How |
|---|---|---|---|
| Total fixture wall / `duration_ms` | measured (table above) | measured | parent binary vs HEAD binary |
| Phase split | **n/a** (uninstrumented) | measured | HEAD only |
| Materialize multiplicity | **2×** source read/winner (structural) | **1×** (`messages_materialized==unique`) | D1 code + integration assert |
| Handle opens / source | separate mat + attach maps | shared LRU, `source_pst_opens==1` | D4 |
| AMap bookkeeping | O(blocks×pages) | O(1) amortized | op-count test |
| Post-write dual hash | sequential digests | concurrent digests | microbench below |

### Phase 5 isolatable microbench — sequential vs concurrent hash (32 MiB)

Machine: same Desktop / debug. Test:
`pst-writer` `concurrent_vs_sequential_hash_timing_32mib` (warmed; digests equal).

| Path | wall_ms (32 MiB file) |
|---|---:|
| Sequential SHA-256 + MD5, 1 MiB buffer (pre-0079 shape) | **1376** |
| Concurrent SHA-256 + MD5, same buffer (`std::thread::scope`) | **909** |

**Result:** concurrent path is ~**1.5×** faster on 32 MiB on this machine; digests match.
On the aspose fixture (~0.2–0.8 MiB written) absolute hash_ms is small (~11–30);
the concurrent rewrite is the multi-GB constant-factor win, measured here where it is visible.

## Cancel latency baseline

0078 cancel/quarantine suite remains the contract gate. No operator multi-GB cancel measurement (D-0079-operator-multigb / D-0079-cancel-latency).

## Complexity baseline (AMap, operation count)

Unit test `amap_scan_steps_linear_in_block_count`: 200 vs 800 blocks; steps/block ratio does not grow superlinearly (asserted in CI). Multi-GB structural win for Phase 2 — not visible as wall on this fixture.

## --jobs skip evidence (fixture + structure)

| Signal | Value |
|---|---|
| Fixture residual after Phases 1–5 | sub-second (HEAD duration_ms ~250–270) |
| Parent vs HEAD fixture | modest duration_ms improvement; both already sub-second |
| D1 | `messages_materialized == unique` |
| D4 | `source_pst_opens == 1` single-source |
| AMap | op-count linear amortized |
| Multi-GB operator | **absent** → do not ship `--jobs` |

Shipping `--jobs` would trade 0077 per-source CRC attribution (`crc_attribution: aggregate` when N>1) without a measured multi-GB miss.
