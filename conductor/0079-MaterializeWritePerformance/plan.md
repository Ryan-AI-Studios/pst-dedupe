# 0079 Plan — Materialize & PST Write Performance

Spec: [`spec.md`](spec.md). Verified against `9c8be49`.

## Locks (do not violate without amending the spec)

1. No measurement, no merge (§2.3.1)
2. Equivalence oracle gates every change (§2.3.2)
3. Fidelity counters are not perf-negotiable (§2.3.3, 0077 rule 2 / 0078 rule 8)
4. Source PSTs read-only; no mmap, no cross-process cache (§2.3.4)
5. Complexity fixes before constant-factor fixes, proven by **operation count** not wall-clock (§2.3.5)
6. One writer thread, always (§2.3.6)
7. Bounded channels only — no unbounded queue (§2.3.7)
8. `--jobs` default 1, opt-in (§2.3.8)
9. No new default dependency without a number (§2.3.9)
10. Timings are data-path, not log-path (§2.3.10)
11. Cancellation must not get slower; 0078 quarantine contract unchanged (§2.3.11)
12. Identity, winner ladder, and write order untouched (§2.3.12)
13. **"Byte-identical output" is not an available test** — D10 (`SystemTime::now()` in the store record key)
14. Phase 5 verification is a fidelity gate; a perf track does not weaken it (§3.7)
15. **No work-stealing runtime for materialize**; no stage re-orders across source boundaries (§1, §3.8)
16. **No mmap of the output temp either** — declined in §3.7 (uncatchable page-fault abort, new default dep, likely hash-bound anyway)

## Phase 0 — Instrument and prove (before any optimization)

- [ ] `PhaseTimings` + `source_pst_opens` / `messages_materialized` / `bytes_written_total` (§3.1)
- [ ] Timings on cancelled runs too; `unaccounted_ms` computed, never zeroed
- [ ] **Export equivalence oracle** (§3.2) as a test helper
- [ ] Oracle self-test: two baseline runs compare equal (proves it tolerates exactly D10's volatile fields)
- [ ] `baseline.md`: per-phase numbers per fixture — **this cannot be reconstructed after the change**
- [ ] Record baseline cancellation latency (lock 11 needs a before)

Phase 0 ships alone and behavior-neutral. It is also the deliverable that makes the 275 s attributable for the first time — valuable even if nothing else in this track lands.

## Phase 1 — D1: materialize each winner once

- [ ] `on_winner` builds `PreparedWinner` and drops the `CanonicalMessage` immediately (§3.3)
- [ ] Delete the second `materialize` in `prepare_winner` (`unique_pst_cmd.rs:2638`)
- [ ] Write order still driven by `keep_set.winners`, not by group order
- [ ] **Reason-set equivalence test** on a fixture with attach fails + CRC-suspect + a promoted winner — the merge point moves (`keepset.rs:2376` vs `unique_pst_cmd.rs:2646-2656`); prove the set is identical, do not inspect it
- [ ] `messages_materialized == keep_set.stats.unique` asserted
- [ ] **By-value `from_canonical_message` (D11)** — move bodies and attach payloads instead of cloning (`production.rs:772-818`). Same change, same moment; doing it later means writing the clone twice
- [ ] `prepared_bytes_peak` reported + soft warning above a documented threshold (§3.9) — stability, not just speed
- [ ] Oracle passes; record the delta

## Phase 2 — D2: O(1) amortized AMap bookkeeping

- [ ] `place_and_write_block`: `stubbed_upto` watermark replaces the per-block full filter (`lib.rs:349-354`)
- [ ] `amap_ensure_page`: `HashSet<u64>` of registered offsets replaces `iter().any()` (`lib.rs:571`)
- [ ] Operation-count instrumentation + assertion across 1× and 4× fixtures (lock 5)
- [ ] Oracle passes — placement, offsets and bytes must be unchanged
- [ ] `review.md` states plainly that this is a **multi-GB** fix, sub-second at INC scale

## Phase 3 — D3: one positioned write per block

- [ ] Measure first: is the seek or the write the cost?
- [ ] Either `eager.file_pos` tracking (skip redundant seeks) or `FileExt::seek_write` / `write_at`
- [ ] **Do not** wrap the eager file in a `BufWriter` while seeks remain (§3.5 — `BufWriter::seek` flushes)
- [ ] AMap stub writes land behind the cursor (`lib.rs:349-358`) — any buffering scheme must handle that explicitly
- [ ] Oracle passes

## Phase 4 — D4/D5: one bounded handle cache

- [ ] `PstHandleCache` with LRU, `--max-open-psts` default 32 (matches the probe path)
- [ ] Resolve the `finalize_with_materialize` borrow conflict (`pst_materializer.rs:490-493`) with `Rc<RefCell<…>>`, **not** by keeping two caches
- [ ] Shared by `PstMaterializer` and `PstAttachStreamSource`
- [ ] `source_pst_opens` drops by the expected count — assert it, don't assume it
- [ ] Eviction exercised with more sources than the cap
- [ ] Close **D-0074-mat-lru** in `deferred.md`

## Phase 5 — D7: post-write passes, measured

- [ ] Report `verify_ms` and the final-hash cost separately
- [ ] **Split the measurement: read-bound or hash-bound?** MD5 (~500 MB/s) is the likely ceiling, not the disk
- [ ] If hash-bound: run SHA-256 and MD5 concurrently over the same buffer — no new dependency
- [ ] Raise the 256 KiB hash buffer only if the measurement supports it; sequential-access hint on Windows if it helps
- [ ] **Do not mmap the temp** (lock 16). Rule 4 does not cover it; §3.7 declines it anyway — a page fault is an uncatchable abort, and the last step before rename is the worst place to lose 0078's exit contract
- [ ] **Do not** weaken Phase 5 verification (lock 14). If it dominates, that is a 0080 finding, not a licence to sample less
- [ ] Narrow D-0070-inline-hash-io with the finalize-seek reason (§2.4) — it is impossible, not merely undone

## Phase 6 — `--jobs` (only if Phases 1–5 miss the target)

- [ ] **Decision gate first:** if Phases 1–5 hit the target, do not build this. Record the numbers and skip to Phase 7
- [ ] Dedicated `std::thread` workers over `std::sync::mpsc::sync_channel` — **not rayon** (lock 15); single writer; ordered commit by keep-set index
- [ ] **Source-affinity partitioning**: each worker gets a *contiguous single-source run*. Items are already source-clustered (`scan.rs:535`), so this is free — and a work-stealing distributor would throw it away and make `--jobs 4` slower than `--jobs 1`
- [ ] Assert `source_pst_opens` does **not** scale with N on a multi-source fixture (this is the test that would have caught the thrash)
- [ ] Document that useful N is bounded by distinct source count, so the flag does not look broken
- [ ] Default 1
- [ ] `crc_attribution: "per_source" | "aggregate"`; under `N > 1` per-source CRC fields **omitted**, never guessed (§3.8, D-0077-parallel-attrib)
- [ ] Cancellation latency re-measured (lock 11)
- [ ] Oracle passes at N = 1, 2, 4 — output identical across all three

## Phase 7 — Docs + registry + gate

- [ ] `review.md`: per-phase before/after, fixture and machine named; why `--jobs` did or did not ship
- [ ] Document `PhaseTimings` in `docs/unique-pst-export.md`
- [ ] `deferred.md`: D-0074-mat-lru **closed**; D-0070-inline-hash-io **narrowed**; D-0073-vec-events **declined with reason**; add D-0079-deterministic-key / -reader-buffer / -stream-prepare / -operator-multigb
- [ ] `conductor.md` + `sequencing.md` rows
- [ ] Operator multi-GB run recorded, or explicitly marked absent
- [ ] `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`

## Suggested order

**Phase 0 is not optional and is not overhead** — it is the only thing that makes every later phase falsifiable, and `baseline.md` cannot be recovered once the code changes. Phases 1–5 are independent of each other and can interleave, but each one lands with its own oracle pass and its own number.

**Phase 1 is the leading candidate for the INC 275 s** (halving per-winner source reads); **Phase 2 is the leading candidate at multi-GB** (the quadratic). They fix different regimes and neither substitutes for the other — resist the temptation to stop after whichever one the fixture happens to reward.

If the track is cut short, the minimum coherent slice is **Phase 0 + Phase 1**: attribution plus the one change that is a pure win with no scale caveat.

## Handoff

**Do:** build the oracle before the first optimization; commit `baseline.md` before touching code; assert complexity by operation count; state the parallel trade in `summary.json`, not in a comment; report the numbers that made a phase unnecessary.

**Do not:** claim a speedup without a phase number; wrap a seeking writer in a `BufWriter`; mmap a source PST *or* the output temp; use rayon or any work-stealing runtime for materialize; let any stage re-order across source boundaries; leave any queue unbounded; ship `--jobs` because it was built; fill per-source CRC fields under parallel materialize; weaken Phase 5 verification; change identity, the winner ladder, or write order; write a DoD that assumes byte-identical output; assume large attachments are the RAM risk — they already stream.
