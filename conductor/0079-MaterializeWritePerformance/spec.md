# 0079 — Materialize & PST Write Performance

- **Track ID:** 0079-MaterializeWritePerformance
- **Status:** Ready
- **Series:** L
- **Evidence:** INC unique-pst ~**275 s** wall; scan alone ~**3 s** — materialize+write is the bottleneck

## 1. Objective

Cut end-to-end unique-pst time on multi-GB, multi-mailbox PSTs via **profiling-led** improvements to materialize and streaming write, without sacrificing determinism or source immutability.

## 2. Context & research

- Current path: sequential scan → resolve → per-winner materialize (re-open messages/attaches) → streaming PST write.
- Materializer re-lists attaches; small attaches buffered ≤64 KiB; large stream at write.
- Writer already chunked/streaming (0070); remaining costs likely: repeated PST open/seek, CRC recompute, single-threaded encode, small writes, lock contention.
- Rust I/O practice: large buffers (`BufWriter` 1–8 MiB), `write_all` batching, avoid per-block flush, optional `memmap2` for **read-only** source maps, parallel **read** of independent winners with ordered write queue.
- Windows: sequential scan hints, large pagefile-friendly buffers; avoid millions of tiny `WriteFile`s.

## 3. In scope

1. **Baseline profiler** for unique-pst: phases timed in JSON (`scan_ms`, `resolve_ms`, `materialize_ms`, `write_ms`, `verify_ms`).
2. **Materialize**
   - Reuse open `PstFile` handles per source (connection pool / sticky open).
   - Optional parallel materialize of winners with **ordered** commit to writer (rayon + channel); default sequential if `--jobs 1`.
   - Prefetch attach metadata once; avoid double list_attachments.
3. **Writer**
   - Ensure large BufWriter / batch AMap updates (measure before change).
   - Reduce redundant CRC hashing if digests only needed at finalize.
4. **Benchmark**: synthetic fixture + operator INC optional; target **≥1.5×** faster on synthetic multi-attach pack; document INC delta if available.
5. No change to keep-set determinism (sort keys unchanged).

## 4. Out of scope

- GPU/SIMD crypto for SHA unless free win.
- Multi-process export.

## 5. Risks

| Risk | Mitigation |
|------|------------|
| Parallel races | Single writer thread; materialize-only parallelism |
| Memory blowup | Bound in-flight winners; stream large attaches |

## 6. DoD

- [ ] Phase timings in unique-pst JSON always
- [ ] `--jobs` materialize parallelism with tests
- [ ] Measured speedup on fixture documented in review.md
- [ ] No fidelity regression vs 0071 tests
