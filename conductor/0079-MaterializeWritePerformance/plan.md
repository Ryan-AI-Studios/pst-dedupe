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

- [x] `PhaseTimings` + `source_pst_opens` / `messages_materialized` / `bytes_written_total` (§3.1)
- [x] Timings on cancelled runs too; `unaccounted_ms` computed, never zeroed
- [x] **Export equivalence oracle** (§3.2) as a test helper
- [x] Oracle self-test: two baseline runs compare equal (proves it tolerates exactly D10's volatile fields)
- [x] `baseline.md`: parent (`9c8be49`) vs HEAD wall/`duration_ms` + HEAD phase split (Codex P1-1)
- [x] Record baseline cancellation latency (lock 11): 0078 behavioral gate retained; numeric multi-GB residual

## Phase 1 — D1: materialize each winner once

- [x] `on_winner` builds `PreparedWinner` and drops the `CanonicalMessage` immediately (§3.3)
- [x] Delete the second `materialize` in `prepare_winner`
- [x] Write order still driven by `keep_set.winners`, not by group order
- [x] **Reason-set / promote / soft-reason** proof: mock hard-fail promote + first-call-only soft reasons; attachments-on attach fails; aspose CRC_SUSPECT
- [x] `messages_materialized == keep_set.stats.unique` asserted
- [x] **By-value `from_canonical_message` (D11)** — move bodies and attach payloads
- [x] `prepared_bytes_peak` reported + soft warning above a documented threshold (§3.9)
- [x] Oracle passes parent vs HEAD; delta in `baseline.md`

## Phase 2 — D2: O(1) amortized AMap bookkeeping

- [x] `place_and_write_block`: `stubbed_upto` watermark replaces the per-block full filter
- [x] `amap_ensure_page`: `HashSet<u64>` of registered offsets replaces `iter().any()`
- [x] Operation-count instrumentation + assertion across 1× and 4× fixtures (lock 5)
- [x] Oracle passes — placement, offsets and bytes must be unchanged
- [x] `review.md` states plainly that this is a **multi-GB** fix, sub-second at INC scale

## Phase 3 — D3: one positioned write per block

- [x] Measure first: is the seek or the write the cost? (positioned path + file_pos)
- [x] Either `eager.file_pos` tracking (skip redundant seeks) or `FileExt::seek_write` / `write_at`
- [x] **Do not** wrap the eager file in a `BufWriter` while seeks remain (§3.5)
- [x] AMap stub writes land behind the cursor — handled via positioned write
- [x] Oracle passes

## Phase 4 — D4/D5: one bounded handle cache

- [x] `PstHandleCache` with LRU, `--max-open-psts` default 32 (matches the probe path)
- [x] Resolve the `finalize_with_materialize` borrow conflict with `Rc<RefCell<…>>`, **not** by keeping two caches
- [x] Shared by `PstMaterializer` and `PstAttachStreamSource`
- [x] `source_pst_opens` drops by the expected count — assert it, don't assume it
- [x] Eviction exercised with more sources than the cap
- [x] Close **D-0074-mat-lru** in `deferred.md`

## Phase 5 — D7: post-write passes, measured

- [x] Report `verify_ms` and the final-hash cost separately
- [x] **Split the measurement: read-bound or hash-bound?** Concurrent SHA-256+MD5 over same buffer
- [x] If hash-bound: run SHA-256 and MD5 concurrently over the same buffer — no new dependency
- [x] Raise the 256 KiB hash buffer only if the measurement supports it; sequential-access hint residual
- [x] **Do not mmap the temp** (lock 16)
- [x] **Do not** weaken Phase 5 verification (lock 14)
- [x] Narrow D-0070-inline-hash-io with the finalize-seek reason (§2.4)

## Phase 6 — `--jobs` (only if Phases 1–5 miss the target)

- [ ] **SKIPPED — decision gate:** Phases 1–5 leave fixture residual **sub-second** (parent vs HEAD measured in `baseline.md`). Multi-GB operator evidence **absent** (`D-0079-operator-multigb`). Shipping `--jobs` would trade 0077 per-source CRC attribution without a measured multi-GB miss. **Do not build** until operator multi-GB proves need. (DoD-13)

## Phase 7 — Docs + registry + gate

- [x] `review.md`: per-phase evidence, fixture and machine named; why `--jobs` did not ship
- [x] Document `PhaseTimings` in `docs/unique-pst-export.md`
- [x] `deferred.md`: D-0074-mat-lru **closed**; D-0070-inline-hash-io **narrowed**; D-0073-vec-events **declined with reason**; add D-0079-* residuals; header **In Progress** until final Codex PASS
- [x] `conductor.md` + `sequencing.md` rows (**In Progress** until final gate)
- [x] Operator multi-GB run recorded as **absent** (residual)
- [ ] `cargo fmt --all --check`; clippy/tests — re-run on P1 fix commit

## Suggested order

**Phase 0 is not optional** — parent-vs-HEAD oracle + `baseline.md` numbers. Phases 1–5 landed; Phase 6 skipped with measurement.

## Handoff

**Do:** keep parent-oracle allowlist honest; re-measure multi-GB before considering `--jobs`; assert complexity by operation count.

**Do not:** claim multi-GB speedups without operator numbers; ship `--jobs` because it was sketched; weaken Phase 5 verification; change identity, winner ladder, or write order.
