# 0079 Review — Materialize & PST Write Performance

| Field | Value |
|---|---|
| Track | 0079-MaterializeWritePerformance |
| Branch | `track/0079-materialize-write-performance` |
| Ledger tx | `c305c426-c4a9-4cf8-9b55-628fcedb5204` (REFACTOR) |
| Machine | Desktop / DESKTOP / Windows NT 10.0.26200.0 |
| Fixture | `fixtures/aspose_outlook.pst` (~3.2 MiB, 17 unique) |
| Parent | `9c8be49` worktree `.wt-0079-baseline` |
| Date | 2026-07-28 |
| Status | **Completed — Codex luna PASS WITH DEFERRED P3** (final gate `review.codex.final.md`) |

## Scope

Make unique-pst faster on multi-GB exports **without changing product semantics**:
single materialize per winner, O(1) AMap bookkeeping, positioned block writes,
one shared bounded PST handle LRU, concurrent post-write hashing, phase
instrumentation + export equivalence oracle. **No** `--jobs`, **no** mmap,
**no** verify weakening.

## Parent baseline + oracle (Codex P1-1)

Phases 0–5 landed as one change set; a pure pre-opt *instrumented* parent does
not exist. Codex required **parent binary vs HEAD** under the structural oracle:

1. Worktree at `C:\dev\Dedupe\.wt-0079-baseline` from `9c8be49`.
2. `cargo build -p pst-dedup-cli` (debug) in worktree + HEAD.
3. `unique-pst` on aspose twice (`--no-attachments` and attachments-on).
4. HEAD `export_oracle::compare_export_packs` — **pass** (allowlist equalizes
   parent packs missing `PhaseTimings` / `messages_materialized` / etc.).
5. Numbers + paths in `baseline.md`. HEAD-only: `messages_materialized == unique`.
6. Optional CI: `PST_DEDUPE_BASELINE_BIN` → `unique_pst_parent_baseline_oracle_when_env_set`.

### Measured parent vs HEAD (warm debug medians)

| Mode | parent duration_ms | HEAD duration_ms | HEAD wall_ms band |
|---|---:|---:|---|
| `--no-attachments` | **265** | **249** | ~281–291 |
| attachments on | **299** | **268** | ~303–309 |

Exact n=3 table: see `baseline.md`. Both remain **sub-second** on the fixture.

## What shipped (Phases 0–5) — structural evidence

### Phase 0 — Instrument
- `PhaseTimings` on `UniqueExportSummary` (`serde(default)`, additive)
- Counters: `source_pst_opens`, `messages_materialized`, `bytes_written_total`,
  `prepared_bytes_peak`, `hash_ms`
- `pst_dedup_cli::export_oracle` structural pack compare (not byte-identical — D10)
- Allowlist docs + unit test for parent-without-0079-fields
- Evidence: `baseline.md`; tests `unique_pst_oracle_self_test_two_runs`,
  `unique_pst_parent_baseline_oracle_when_env_set` (env-gated)

### Phase 1 — D1 single materialize + D11 by-value
- `on_winner` → `PreparedWinner` via `from_canonical_message_owned` (move bodies/payloads)
- Prepare is pure re-order by keep-set item index — **no second materialize**
- Missing prepared winners **hard-fail before write** (unless cancel)
- Evidence: `unique_pst_messages_materialized_equals_unique`;
  mock `promote_first_materialize_soft_reasons_only_no_second_call_pollution`

### Phase 2 — O(1) AMap
- `stubbed_upto` watermark; `amap_page_offsets: HashSet<u64>`
- Evidence: unit test `amap_scan_steps_linear_in_block_count` (200 vs 800 blocks)

### Phase 3 — Positioned writes
- `file_pos` + `FileExt::seek_write` / `write_at`; **no** `BufWriter` on eager seek path
- Evidence: code review of `EagerWriteCtx` write path

### Phase 4 — PstHandleCache (closes D-0074-mat-lru)
- Shared `Rc<RefCell<PstHandleCache>>` (default 32, `--max-open-psts`)
- Evidence: eviction unit test; fixture `source_pst_opens == 1`

### Phase 5 — Post-write hash
- Concurrent SHA-256 + MD5 over same 1 MiB buffer (`std::thread::scope`)
- Phase 5 `verify_volumes` unchanged; `hash_ms` / `verify_ms` reported
- mmap declined (sources and output temp)
- Evidence: `concurrent_hash_file_hex_matches_sequential` (correctness);
  `concurrent_vs_sequential_hash_timing_32mib` (**measured**: sequential **1376 ms** vs
  concurrent **909 ms** on 32 MiB, same machine; digests equal)

## Why `--jobs` was **not** shipped (DoD-13) — measured skip

After Phases 1–5:

| Signal | Value |
|---|---|
| Fixture residual (debug HEAD) | **sub-second** (`duration_ms` ~250–270) |
| Parent vs HEAD fixture | modest win; parent already sub-second |
| D1 | `messages_materialized == unique` (double source read gone) |
| AMap | operation-count linear amortized (multi-GB scale fix) |
| D4 / handle opens | shared LRU; single-source opens == 1 |
| Operator multi-GB | **residual** `D-0079-operator-multigb` — **not measured**; no invented numbers |

**Decision:** `--jobs` **not shipped** because fixture residual is already
sub-second after Phases 1–5 structural fixes. Shipping `--jobs` would trade
0077 per-source CRC attribution (`crc_attribution: aggregate` when N>1) without
a measured multi-GB miss. Revisit only after `D-0079-operator-multigb` proves
Phases 1–5 miss the operator target.

## DoD-7 fidelity (Codex P1-2)

1. **Mock materializer** (`dedup-engine`): peer A hard-fails → promote B;
   second materialize of B would add extra soft reason; keep-set / on_winner
   see **first-call only**; count == 1 unique.
2. **Attachments-on path:** aspose yields `attachments_failed=4`
   (`ATTACH_EMBEDDED_UNPARSED`); degraded reason maps non-empty + stable across
   two runs (`unique_pst_attachments_on_degraded_and_attach_fails_stable`).
3. **CRC:** aspose winners carry `CRC_SUSPECT` (scan/finalize path); same
   single-materialize merge as attach soft reasons. Mock covers double-call
   divergence class. No separate CRC-suspect-only fixture required beyond aspose
   + mock.

## Cancel latency (DoD-16 honesty)

- **Behavioral gate retained:** 0078 `export_exit_0078` suite.
- **Numeric cancel latency** on multi-GB: residual with operator multi-GB.

## DoD matrix (abbreviated)

| DoD | Status | Residual |
|---|---|---|
| 1 PhaseTimings + unaccounted_ms | **Met** | unaccounted large on short fixtures (honest) |
| 2 Counters reported | **Met** | |
| 3 Oracle + parent gate | **Met** | parent worktree + env optional test + allowlist |
| 4 baseline.md | **Met** | parent vs HEAD numbers + machine |
| 5 messages_materialized == unique | **Met** | HEAD only field |
| 6 second materialize gone | **Met** | |
| 6a by-value convert | **Met** | |
| 6b prepared_bytes_peak + warn | **Met** | |
| 7 reason-set / promote / attach / CRC | **Met** | mock + attachments-on + aspose CRC_SUSPECT |
| 8 AMap O(1) op-count | **Met** | |
| 9 positioned writes / no BufWriter | **Met** | |
| 10 shared LRU + max-open-psts | **Met** | D-0074-mat-lru closed |
| 11 verify not weakened; hash/verify ms | **Met** | |
| 12/12a --jobs | **N/A** | not shipped (DoD-13) |
| 13 why no --jobs | **Met** | measured fixture + structural + multi-GB residual |
| 14 measured speedup | **Met (fixture + isolatable)** | parent vs HEAD wall; HEAD phase split; 32 MiB hash seq vs conc; multi-GB residual |
| 15 0071/73/74/77/78 suites | **Met at commit** | pre-commit full suite green on P1 commit; re-verify below |
| 16 cancel latency | **Partial** | behavioral 0078 retained; numeric residual |
| 17 no unjustified default dep | **Met** | |
| 18 deferred.md | **Met** | In Progress header until final PASS |
| 19 conductor/sequencing/review | **Met** | Completed after final Codex PASS WITH DEFERRED P3 |
| 20 fmt/clippy/test | **Gate** | orchestrator re-runs outside Codex sandbox (sandbox cannot write `target/`) |

## Residuals

- `D-0079-operator-multigb` — operator multi-GB before/after with phase split (**blocks shipping --jobs**)
- Numeric cancel latency on multi-GB (DoD-16)
- `D-0079-seq-scan` — optional Windows sequential-scan at CreateFile
- `D-0079-stream-prepare` — streaming prepare→write when peak warns
- Per-phase wall attribution of historical INC 275 s forever missing (parent was uninstrumented)

## Gate commands (orchestrator-writable environment)

Codex read-only sandbox cannot take the cargo lock under `target/debug` — full
gates are re-run by the orchestrator and recorded here after each fix commit.

```powershell
cargo fmt --all --check
cargo test -p dedup-engine --lib
cargo test -p pst-dedup-cli --lib
cargo test -p pst-dedup-cli --test unique_pst
cargo test -p pst-writer --lib
cargo check -p pst-dedup-gui
cargo clippy -p pst-dedup-cli -p pst-writer -p pst-dedup-gui -p dedup-engine --all-targets -- -D warnings
cargo test --workspace   # full DoD-20 when time permits
```

### Gate results (hash timing + docs fix commit)

| Command | Result |
|---|---|
| `cargo fmt --all` | ok |
| `cargo test -p pst-writer --lib` | **13 passed** (incl. 32 MiB hash timing) |
| `cargo test -p pst-dedup-cli --test unique_pst` | **24 passed** |
| `cargo check -p pst-dedup-gui` | ok |
| `cargo clippy -p pst-dedup-cli -p pst-writer -p pst-dedup-gui -p dedup-engine --all-targets -- -D warnings` | ok |
| 32 MiB hash microbench | sequential **1376 ms** / concurrent **909 ms** |
