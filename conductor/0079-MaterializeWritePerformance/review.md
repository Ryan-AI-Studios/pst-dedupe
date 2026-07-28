# 0079 Review — Materialize & PST Write Performance

| Field | Value |
|---|---|
| Track | 0079-MaterializeWritePerformance |
| Branch | `track/0079-materialize-write-performance` |
| Ledger tx | `c305c426-c4a9-4cf8-9b55-628fcedb5204` (REFACTOR) |
| Machine | Desktop / Windows NT 10.0.26200.0 |
| Fixture | `fixtures/aspose_outlook.pst` (~3.2 MiB, 17 unique) |
| Date | 2026-07-28 |

## Scope

Make unique-pst faster on multi-GB exports **without changing product semantics**:
single materialize per winner, O(1) AMap bookkeeping, positioned block writes,
one shared bounded PST handle LRU, concurrent post-write hashing, phase
instrumentation + export equivalence oracle. **No** `--jobs`, **no** mmap,
**no** verify weakening.

## Honesty: pre-opt instrumented baseline

Phases 0–5 landed as one change set on this branch. A pure pre-optimization
instrumented run on this machine is **not reconstructible**. Do **not** invent
per-phase attribution of the historical INC ~**275 s** wall (scan ~3 s, 3728
winners, 366 attach fails — operator evidence; source PSTs not in git).

What we can claim:

1. **Structural pre-D1:** every winner was materialized twice
   (`finalize_with_materialize` + prepare re-materialize).
2. **Structural post-D1:** `messages_materialized == unique` (fixture 17==17).
3. **Post-change fixture phase split** in `baseline.md` (attributable going
   forward). `unaccounted_ms` is honest setup overhead on short runs — not
   zeroed.

## What shipped (Phases 0–5) — structural evidence

### Phase 0 — Instrument
- `PhaseTimings` on `UniqueExportSummary` (`serde(default)`, additive)
- Counters: `source_pst_opens`, `messages_materialized`, `bytes_written_total`,
  `prepared_bytes_peak`, `hash_ms`
- `pst_dedup_cli::export_oracle` structural pack compare (not byte-identical — D10)
- Evidence: `baseline.md`; test `unique_pst_oracle_self_test_two_runs`

### Phase 1 — D1 single materialize + D11 by-value
- `on_winner` → `PreparedWinner` via `from_canonical_message_owned` (move bodies/payloads)
- Prepare is pure re-order by keep-set item index — **no second materialize**
- Missing prepared winners **hard-fail before write** (unless cancel)
- Evidence: `unique_pst_messages_materialized_equals_unique` asserts
  `messages_materialized == unique` and `source_pst_opens == 1`

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
- Evidence: `concurrent_hash_file_hex_matches_sequential` (pst-writer)

## Why `--jobs` was **not** shipped (DoD-13)

After Phases 1–5 on the fixture:

| Signal | Value |
|---|---|
| Fixture wall (debug) | ~300 ms residual |
| D1 | `messages_materialized == unique` (double source read gone) |
| AMap | operation-count linear amortized (multi-GB scale fix) |
| D4 / handle opens | shared LRU; single-source opens == 1 |
| Operator multi-GB | **residual** `D-0079-operator-multigb` (not measured here) |

Shipping `--jobs` would trade 0077 per-source CRC attribution
(`crc_attribution: aggregate` when N>1) for a win fixtures do not require and
that multi-GB operator data has not yet shown is needed. Revisit only after
`D-0079-operator-multigb` proves Phases 1–5 miss the target.

## Cancel latency (DoD-16 honesty)

- **Behavioral gate retained:** 0078 `export_exit_0078` suite (exit 130,
  quarantine, `artifact_state`) remains the contract.
- **Numeric cancel latency** (ms before/after on multi-GB mid-write) is **not**
  measured on this machine — residual with operator multi-GB. No code path was
  introduced that intentionally defers cancel checks across long non-checkpoint
  regions for `--jobs` (jobs not shipped).

## DoD matrix (abbreviated)

| DoD | Status | Residual |
|---|---|---|
| 1 PhaseTimings + unaccounted_ms | **Met** | unaccounted large on short fixtures (honest) |
| 2 Counters reported | **Met** | |
| 3 Oracle helper + self-test | **Met** | structural only (D10) |
| 4 baseline.md | **Partial** | post-change only; pre-opt instrumented N/A |
| 5 messages_materialized == unique | **Met** | |
| 6 second materialize gone | **Met** | |
| 6a by-value convert | **Met** | |
| 6b prepared_bytes_peak + warn | **Met** | |
| 7 reason-set equivalence | **Partial** | post-D1 stability test + finalize-only structural proof; pre-D1 binary reason compare not reconstructible |
| 8 AMap O(1) op-count | **Met** | |
| 9 positioned writes / no BufWriter | **Met** | |
| 10 shared LRU + max-open-psts | **Met** | D-0074-mat-lru closed |
| 11 verify not weakened; hash/verify ms | **Met** | |
| 12/12a --jobs | **N/A** | not shipped (DoD-13) |
| 13 why no --jobs | **Met** | this document |
| 14 measured speedup per phase | **Partial** | structural wins + fixture split; no false 275s phase attribution |
| 15 0071/73/74/77/78 suites | **Met** when gate green | re-run commands below |
| 16 cancel latency | **Partial** | behavioral 0078 retained; numeric residual |
| 17 no unjustified default dep | **Met** | |
| 18 deferred.md | **Met** | |
| 19 conductor/sequencing/review | **Met** | |
| 20 fmt/clippy/test workspace | **Gate** | see verification |

## Review fixes landed post-implementation (this pass)

1. GUI `UniquePstCliArgs` missing `max_open_psts` (compile break)
2. DoD-7 test via `degraded_reasons_by_winner` + D1 structural proof
3. D1 asserts `source_pst_opens == 1`; attachments-on oracle/D1; concurrent hash unit test
4. Prepare incomplete → hard-fail before write (unless cancel)
5. Removed fake `FILE_FLAG_SEQUENTIAL_SCAN` claim on `hash_file_hex`
6. `.gitignore`: root scratch is `/review.md` only; conductor stays `/conductor/`
   (historical force-add policy). Orchestrator: `git add -f` baseline,
   implementation-notes, review.md

## Residuals

- `D-0079-operator-multigb` — operator multi-GB before/after with phase split
- Numeric cancel latency on multi-GB (DoD-16)
- `D-0079-seq-scan` — optional Windows sequential-scan at CreateFile (not std-open)
- `D-0079-stream-prepare` — streaming prepare→write when `prepared_bytes_peak` warns
- Pre-opt instrumented baseline forever missing on this branch (documented)

## Gate commands to re-run

```powershell
cargo fmt --all --check
cargo check -p pst-dedup-gui
cargo test -p pst-writer --lib
cargo test -p pst-dedup-cli --lib
cargo test -p pst-dedup-cli --test unique_pst
cargo test -p pst-dedup-cli --test export_exit_0078
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
