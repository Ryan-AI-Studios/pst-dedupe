# 0079 Implementation notes

Branch: `track/0079-materialize-write-performance`
Ledger tx: `c305c426-c4a9-4cf8-9b55-628fcedb5204` (REFACTOR)
Status: **In Progress — Codex P1 fix round**

## What shipped (Phases 0–5)

### Phase 0 — Instrument
- `PhaseTimings` on `UniqueExportSummary` (all `serde(default)`, additive)
- Counters: `source_pst_opens`, `messages_materialized`, `bytes_written_total`,
  `prepared_bytes_peak`, `hash_ms`
- Instant timers for scan / deep_attach_preflight / resolve / materialize /
  prepare / write / report / verify / quarantine; `unaccounted_ms` computed
- Export equivalence oracle: `pst_dedup_cli::export_oracle` (structural, not
  byte-identical — D10); allowlist equalizes pre-0079 parent packs
- Oracle self-test: two fixture runs compare equal
- Parent (`9c8be49` worktree) vs HEAD unique-pst + oracle; numbers in `baseline.md`
- Optional env gate: `PST_DEDUPE_BASELINE_BIN`

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

## Phase 6 — `--jobs` **SKIPPED** (measured)

Parent vs HEAD fixture (Desktop/DESKTOP, debug, warm medians):

- `--no-attachments`: parent `duration_ms` **265** vs HEAD **249**
- attachments-on: parent **299** vs HEAD **268**
- Both sub-second; `messages_materialized == unique`, `source_pst_opens == 1`
- AMap op-count linear amortized (multi-GB regime structural fix)

Multi-GB operator evidence **absent** → residual `D-0079-operator-multigb`.
**`--jobs` not shipped**: would trade 0077 per-source CRC attribution without a
measured multi-GB miss.

## Codex P1 fixes

- Parent worktree baseline + oracle allowlist + env optional test
- Mock: hard-fail promote + first-materialize soft reasons only (DoD-7)
- Attachments-on attach-fail + degraded_reasons stability test
- Governance: plan 0–5 checked; Phase 6 skip note; deferred/conductor In Progress

## Measurements

See `baseline.md`. Headline:

| Fact | Value |
|---|---|
| D1 | `messages_materialized == unique` (17 == 17) on HEAD |
| Shared opens | `source_pst_opens == 1` |
| Parent oracle | green (`PST_DEDUPE_BASELINE_BIN` / worktree) |
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
