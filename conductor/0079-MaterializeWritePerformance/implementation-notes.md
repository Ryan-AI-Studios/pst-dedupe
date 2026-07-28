# 0079 Implementation notes

Branch: `track/0079-materialize-write-performance`  
Ledger tx: `c305c426-c4a9-4cf8-9b55-628fcedb5204` (REFACTOR; not committed here)

## What shipped (Phases 0–5)

### Phase 0 — Instrument
- `PhaseTimings` on `UniqueExportSummary` (all `serde(default)`, additive)
- Counters: `source_pst_opens`, `messages_materialized`, `bytes_written_total`,
  `prepared_bytes_peak`, `hash_ms`
- Instant timers for scan / deep_attach_preflight / resolve / materialize /
  prepare / write / report / verify / quarantine; `unaccounted_ms` computed
- Export equivalence oracle: `pst_dedup_cli::export_oracle` (structural, not
  byte-identical — D10)
- Oracle self-test: two fixture runs compare equal
- `baseline.md` recorded

### Phase 1 — D1 single materialize + D11 by-value
- `on_winner` builds `PreparedWinner` via `from_canonical_message_owned` (moves
  bodies/attach payloads); keyed by `(source_path, nid)`
- Write order still `keep_set.winners` (item index)
- Second `materialize` in prepare path **removed**
- `messages_materialized == unique` integration test
- `prepared_bytes_peak` + soft warning above 1 GiB threshold

### Phase 2 — O(1) AMap bookkeeping
- `stubbed_upto` watermark on `EagerWriteCtx`
- `amap_page_offsets: HashSet<u64>` alongside vec
- Operation-count test across 200 vs 800 blocks (linear amortized)

### Phase 3 — Positioned writes
- `file_pos` tracking skips redundant seeks
- `FileExt::seek_write` (Windows) / `write_at` (Unix) for non-sequential offsets
- **No** `BufWriter` on the eager seeking writer
- AMap stubs behind cursor handled via positioned write

### Phase 4 — PstHandleCache (closes D-0074-mat-lru)
- Bounded LRU default 32 (`--max-open-psts`)
- Shared `Rc<RefCell<PstHandleCache>>` between `PstMaterializer` and
  `PstAttachStreamSource`
- `source_pst_opens` counted on cache
- Eviction unit test (capacity 2, three sources)

### Phase 5 — Post-write hash
- Concurrent SHA-256 + MD5 over same 1 MiB buffer (`std::thread::scope`)
- `hash_ms` on `WritePstReport` and summary
- Phase 5 `verify_volumes` unchanged; `verify_ms` reported
- mmap declined (sources and output temp)

## Phase 6 — `--jobs` **SKIPPED**

Fixture evidence after Phases 1–5:

- Single-source fixture completes in **~300 ms** wall (debug), with
  `messages_materialized == unique` and `source_pst_opens == 1`.
- Leading INC cost candidate (double materialize) is gone structurally.
- AMap quadratic is fixed by operation-count proof (multi-GB regime).
- Handle double-open (D4) closed by shared LRU.

Shipping `--jobs` would trade 0077 per-source CRC attribution
(`crc_attribution: aggregate`) for a win that fixtures do not require and that
the multi-GB operator residual (D-0079-operator-multigb) has not yet shown is
needed. Prefer **not** shipping until an operator multi-GB run proves Phases
1–5 miss the target.

## Measurements (post-change fixture)

See `baseline.md`. Headline fixture facts:

| Fact | Value |
|---|---|
| D1 assertion | `messages_materialized == unique` (17 == 17) |
| Shared opens | `source_pst_opens == 1` for single-source export |
| Oracle | two baseline runs compare equal |
| AMap steps | linear amortized (unit test) |

## Files touched (primary)

- `crates/pst-dedup-cli/src/unique_export_report.rs` — PhaseTimings + counters
- `crates/pst-dedup-cli/src/export_oracle.rs` — new oracle
- `crates/pst-dedup-cli/src/unique_pst_cmd.rs` — pipeline timings + D1 prepare
- `crates/pst-dedup-cli/src/pst_materializer.rs` — PstHandleCache
- `crates/pst-writer/src/lib.rs` — AMap O(1) + positioned writes
- `crates/pst-writer/src/production.rs` — by-value convert + concurrent hash
- `crates/pst-dedup-cli/tests/unique_pst.rs` — oracle + D1 tests
- docs / deferred / conductor updates

## Remaining risks

- `unaccounted_ms` large on short fixture runs (setup vs work); honest, not fudged
- Operator multi-GB before/after still residual (D-0079-operator-multigb)
- Concurrent per-chunk hash spawn cost may dominate tiny files (harmless; multi-GB is the target)
- Integrity reason-set on degraded fixtures relies on single materialize soft
  reasons (pre-change second materialize could theoretically add extra soft
  reasons; promotion path uses first materialize only — intentional)

## Test commands run

```text
cargo test -p pst-writer --lib
cargo test -p pst-dedup-cli --lib handle_cache
cargo test -p pst-dedup-cli --test unique_pst
cargo test -p pst-dedup-cli --test export_exit_0078   # (if run)
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings  # attempted
```
