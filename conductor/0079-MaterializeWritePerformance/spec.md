# 0079 — Materialize & PST Write Performance

- **Track ID:** 0079-MaterializeWritePerformance
- **Status:** Completed
- **Series:** L (unique export hardening)
- **Verified against:** `9c8be49` (post-0078)
- **Evidence:** INC unique-pst ~**275 s** wall, scan ~**3 s**, 3728 winners, 366 attach fails. Source PSTs stay out of git.

## 1. Objective

Make unique-pst fast enough to be usable on multi-GB, multi-mailbox exports **without changing what it produces**. Every change in this track is gated on a measurement and on an equivalence proof against a pre-change baseline.

The 275 s figure has never been attributed to a phase. That is the first defect, not a footnote: the track cannot honestly claim a speedup it cannot locate.

| Capability | Today | After 0079 |
|---|---|---|
| Where the time goes | unknown (`duration_ms` total only) | per-phase ms in `summary.json`, always |
| Winner source reads | 2× per winner | 1× per winner |
| AMap bookkeeping | O(blocks × amap_pages) per volume | O(1) amortized per block |
| Block write syscalls | seek + write per ≤8 KiB block | one positioned write per block |
| Source PST handles | 2 unbounded sticky maps + probe LRU | one bounded LRU cache |
| Proof a change is safe | manual reading | export equivalence oracle in CI |
| Parallel materialize | none | opt-in `--jobs N`, default 1, honest CRC attribution |

### Industry anchors

- `BufWriter::seek` flushes the buffer before seeking ([std docs](https://doc.rust-lang.org/std/io/struct.BufWriter.html)), so wrapping a seek-per-write hot loop in a `BufWriter` buys close to nothing. Positioned writes (`std::os::windows::fs::FileExt::seek_write`, `std::os::unix::fs::FileExt::write_at`) fuse the seek and the write into one syscall and leave the cursor semantics explicit.
- Bounded channels are the standard backpressure mechanism for a producer/consumer pipeline in Rust; unbounded queues convert a throughput problem into a memory problem. `std::sync::mpsc::sync_channel` is bounded and in std — prefer it over adding `crossbeam-channel`/`flume` unless a measurement demands otherwise (rule 9).
- **`rayon` is recommended against for materialize**, despite being current (1.12.0, 2026-04-14). Its work-stealing scheduler is tuned for CPU-bound data parallelism; materialize is random-seek I/O across possibly network-attached storage, where work-stealing produces chaotic disk queueing and — critically — **destroys the source locality this pipeline already has for free** (§3.8). Dedicated `std::thread` workers with explicit blocking behavior are more predictable for random disk I/O. The repo has precedent: D-0019-04 already forbids rayon on the Matter path.
- `divan` 0.1.21 (2025-04-10) and `criterion` are the microbenchmark options. **Neither is adopted here**: the primary instrument is in-product phase timing, which measures the real workload on operator data instead of a synthetic loop. Revisit only if §3 isolates a single hot function that phase timers cannot resolve.

## 2. Current state

### 2.1 What exists today (verified)

- Pipeline (`unique_pst_cmd.rs:4`): integrity scan → optional phase-1b deep attach preflight → `resolve_groups` → `finalize_with_materialize` → prepare winners → multi-volume streaming write → report pack → verify.
- Sticky PST handles **already exist** — `PstMaterializer.psts: HashMap<String, PstFile>` (`pst_materializer.rs:28`) and `PstAttachStreamSource.psts` (`:495`). The placeholder spec's "add sticky opens" bullet is already shipped; the open work is bounding and unifying them.
- Attach payloads ≤ 64 KiB are buffered; larger ones stream at write time (`pst_materializer.rs:285`).
- The writer is already streaming and eager-spilling (0070): `Layout::push_leaf_block` places and writes each leaf to a same-dir temp and drops the `Vec` (`pst-writer/src/lib.rs:365-380`).
- Only one timing exists: `duration_ms = started.elapsed()` (`unique_pst_cmd.rs:1111, 2267`).

### 2.2 Defects

**D1 — every winner is materialized twice.**
`finalize_with_materialize` materializes each group's winner and passes it to `on_winner` (`keepset.rs:2370-2388`). unique-pst passes `&mut |_msg| Ok(())` (`unique_pst_cmd.rs:1715`) — the result is **discarded**. `prepare_winner` then calls `mat.materialize(&entry.locus)` again for every keep-set winner (`:2638-2640`, comment: `"re-materialize"`). Each materialize is a full `read_message_extract` (body decode, XBLOCK assembly, block CRC) plus `list_attachments` plus, under the default family policy, a payload read of every attachment ≤ 64 KiB. The per-winner source read cost is exactly doubled.

This is structural, not a slip: `finalize_with_materialize` iterates **group** order (`keepset.rs:2353`) while `to_keep_set` builds `winners` in **item index** order (`keepset.rs:1875`). A naive streaming handoff would change write order, i.e. change the artifact.

**D2 — AMap stub bookkeeping is quadratic in output size.**
`Layout::place_and_write_block` rescans the entire `eager.amap_pages` vector on **every block write** (`lib.rs:349-354`), probing a `HashSet` per element. `amap_ensure_page` does the same shape with `iter().any()` (`lib.rs:571`). With `AMAP_INTERVAL = 253_952` and `MAX_BLOCK_DATA = 8_176`, an N-byte volume has ≈ N/253 952 pages and ≈ N/8 192 blocks, so the scan costs ≈ N²/2.08×10¹² probes: ~2.1×10⁹ at 2 GB, ~8.3×10⁹ at 4 GB.

Honest scaling note: at INC scale this is sub-second and is **not** the cause of the 275 s. It is a scale defect that dominates exactly at the multi-GB target this track exists to serve. Both vectors are append-at-tail-only, so both fix without touching layout.

**D3 — the eager writer is an unbuffered `File` with a seek before every block.**
`EagerWriteCtx.file: File` (`lib.rs:175`); `place_and_write_block` issues `seek` then `write_data_block` per ≤ 8 176-byte block (`lib.rs:359-360`). Two syscalls per 8 KiB of output.

**D4 — each source PST is opened, and its whole NBT+BBT built, 3–4× per run.**
`PstFile::open` walks and materializes the complete Node BTree and Block BTree (`pst-reader/src/lib.rs:102-131`). Per run a source is opened by: scan, phase-1b probe (opt-in, has its own LRU), `PstMaterializer`, and again by `PstAttachStreamSource`. The last two are separate by design — `finalize_with_materialize` holds an exclusive borrow on the materializer while `on_winner` runs (`pst_materializer.rs:490-493`) — which is a borrow-checker workaround paying a full BTree build.

**D5 — neither sticky map is bounded (D-0074-mat-lru).**
Both are plain `HashMap<String, PstFile>` that only grow. The probe path already has an LRU (default 32). With many custodian PSTs this is unbounded OS handles *and* unbounded NBT/BBT resident memory.

**D6 — every winner's full `WriteMessage`, bodies included, is held in RAM before any byte is written.**
`prepared: Vec<PreparedWinner>` is filled completely (`:1732-1751`) before Phase 3 starts. 0070 closed the writer's own DTO pre-collect (D-0070-dto-collect) but explicitly left "fat in-memory bodies on DTOs remain the caller's responsibility" — this is that caller. It also serializes the pipeline: no write overlaps any read.

**Where the exposure actually is** (this matters, because the intuitive answer is wrong): large attachments are *already* safe. Only payloads ≤ 64 KiB are buffered; anything larger carries `data: None` and streams at write time (`pst_materializer.rs:285`, `production.rs:178-196`). A run with a thousand 50 MB attachments will not OOM here. The unbounded surfaces are (a) `body_plain: Option<String>` and `body_html: Option<Vec<u8>>`, which have **no cap at all** — HTML bodies with inlined base64 images are routine in real mail — and (b) the *aggregate* of the ≤ 64 KiB buffers: 1 M winners × 64 KiB is 64 GB. The cap bounds each attachment, not the sum.

**D11 — `from_canonical_message` deep-copies every winner, including buffered payloads, for no reason.**
It clones every field — `a.data.clone()` per attachment, `body_plain.clone()`, `body_html.clone()` (`production.rs:772-818`) — into a `WriteMessage`, while the source `CanonicalMessage` is dropped immediately afterward (`unique_pst_cmd.rs:2658`). So every winner's body and every buffered attach payload is memcpy'd once with both copies briefly resident, and the survivor is retained in `prepared` for the whole run. A by-value conversion removes the copy and halves peak transient RAM at the same time.

**D7 — the output is fully re-read at least twice after it is written.**
`hash_file_hex` re-reads the finalized temp end to end for SHA-256 + MD5 with a 256 KiB buffer (`production.rs:1559, 1616-1629`). Phase 5 `verify_volumes` then opens each volume — another full NBT/BBT build — and calls `read_message_properties` for **every** message in the volume (`unique_pst_cmd.rs:2895-2905`; its own comment concedes the O(messages) cost). `--verify-hash` adds a third pass.

**D8 — there is no phase timing, so no claim in this track is currently falsifiable.**

**D9 — `finalize_with_materialize` clones the group structure per run** (`resolved.groups.clone()` `:2353`, `group.clone()` `:2359`). Named for completeness; measure before touching.

**D10 — unique-pst output is not reproducible today, so "byte-identical" cannot be the safety net.**
`generate_store_record_key` derives the store's `PidTagRecordKey` and every folder EntryID's ProviderUID from `SystemTime::now()` + process id + path + message count (`production.rs:3069-3095`). Two runs over identical inputs produce different bytes and different reported `sha256_hex`/`md5_hex`. Any DoD phrased as "byte-identical output" would be unsatisfiable. See §3.2.

### 2.3 Locked rules

1. **No measurement, no merge.** Every optimization lands with a before/after number from the same harness, recorded in `review.md`. A change that is "obviously faster" and unmeasured is out of scope.
2. **Equivalence before speed.** No change merges until the §3.2 oracle proves the export is unchanged. Speed is worthless on a different artifact.
3. **Fidelity counters are not perf-negotiable.** No optimization may drop, batch away, approximate, or reorder an integrity counter, an attach ledger row, a `soft_reason`, or an `export_risk` input. Carries 0077 rule 2 and 0078 rule 8: **these live in the data path, and the data path is not the fast path's to trade.**
4. **Source PSTs stay read-only.** No memory-mapping of sources, no cache that outlives the process, no handle that could take a write lock. *Scope, stated so it is not re-litigated:* this rule governs **sources**. The process-owned output temp is not a source and is not covered — it is nonetheless declined for mmap on independent grounds in §3.7.
5. **Complexity before constants.** An O(n²) is a scale-correctness defect and lands before any constant-factor work, proven by an **operation-count** test — never by wall-clock, which is flaky in CI.
6. **One writer thread, always.** Parallelism may only exist upstream of the writer.
7. **Bounded in flight.** Any parallel or pipelined stage uses a bounded channel. No unbounded queue, ever — that converts a speed problem into an OOM.
8. **Parallelism is opt-in and default off** (`--jobs 1`), because it costs a 0077 invariant (see §3.8) and multiplies D4.
9. **No new default dependency without a measured win.** `rayon`, `memmap2`, `divan` are candidates that must earn entry with a number.
10. **Timings are data-path.** Phase timings reach `summary.json` whether or not stderr is attached (0077 rule 2 / 0078 rule 8).
11. **Cancellation must not get slower.** Pipelining must not increase observed Ctrl-C latency, and 0078's quarantine contract (`artifact_state`, exit 130) holds unchanged.
12. **Nothing here changes identity.** `content_hash`, `message_id_norm`, `edrm_mih_hex`, `decided_by`, and the winner ladder are untouched. This track moves *when* work happens, never *what* it decides.

### 2.4 Rolled-in deferred items

| ID | Disposition |
|---|---|
| **D-0074-mat-lru** | **Closed** by §3.6 — one bounded LRU handle cache replaces both unbounded maps. |
| **D-0077-parallel-attrib** | **Addressed** by §3.8 — `--jobs > 1` degrades per-source CRC attribution *explicitly and visibly* rather than reporting wrong per-source numbers. Closed only if parallel materialize ships; otherwise stays open with the honesty field specified. |
| **D-0070-inline-hash-io** | **Narrowed, not closed.** True inline hashing is **impossible** as the writer is built: finalize seeks back to rewrite header, AMap pages, NBT and BBT (`production.rs:1524-1540`), so the final bytes do not exist until after those seeks. What 0079 owns is the buffer size and sequential-read hint (§3.7); the residual shrinks to "restructure finalize so the file is written strictly forward," which is a writer-format track, not this one. |
| **D-0070-operator-multigb** | Carried into **D-0079-operator-multigb** — same harness, now with numbers attached. |
| **D-0076-operator-perf** | Satisfied by the §3.2/§8 harness (fixture-scale in CI; multi-GB operator-local). |
| **D-0073-vec-events** | **Declined, with reason.** 0077 already bounded it (first-N cap 1000 + `_truncated`/`_total`). Converting a bounded 1000-element `Vec` to a channel adds a thread and a failure mode to save at most a few hundred KiB. Recorded so it is not re-raised as free. |
| **D-0066-disk-groups** | **Not rolled in.** Matter-scale candidate store, different owner and different data path. |
| **D-0018-01** | **Not rolled in.** `extract-pst` adapter path, not the unique-pst materializer. |

## 3. Design

### 3.1 Phase timing contract

New `PhaseTimings` on the unique-pst summary, all `#[serde(default)]` and additive (0078 JSON discipline):

```rust
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PhaseTimings {
    pub scan_ms: u64,
    pub deep_attach_preflight_ms: u64,
    pub resolve_ms: u64,
    pub materialize_ms: u64,
    pub prepare_ms: u64,
    pub write_ms: u64,
    pub report_ms: u64,
    pub verify_ms: u64,
    pub quarantine_ms: u64,
    /// total_ms − Σ(phases). Non-zero is a gap in the instrumentation, not noise.
    pub unaccounted_ms: u64,
    pub total_ms: u64,
}
```

Alongside, the counters that make the timings interpretable: `source_pst_opens`, `messages_materialized` (must equal `unique` after §3.3 — the D1 assertion in production form), `bytes_written_total`.

Rules: monotonic `Instant` only; phases reached before a cancel still report; `unaccounted_ms` is computed, never fudged to zero.

### 3.2 The export equivalence oracle (Phase 0, before any optimization)

Because of D10 the artifact is not byte-reproducible, so the oracle compares **structure and content**, not bytes:

1. `keep_set.json`, `decision.csv`, `export_messages.csv`, `export_attachments.csv` — exact match, modulo an explicitly enumerated timing/path field allowlist.
2. `summary.json` — exact match modulo `duration_ms`, `PhaseTimings`, and the hash fields.
3. Per volume: message count, folder tree (paths + order), and for every message in traversal order a digest over (`normalized MID`, `subject`, `body_plain`, `body_html`, ordered attach `(filename, size, mime, sha256-of-bytes)`).
4. Integrity: `degraded_reasons` per winner, attach ledger rows, and every 0077 CRC counter — identical.

This oracle is a **deliverable in its own right** and is the reason the rest of the track can move quickly. It is built and proven to pass against itself *before* a single optimization lands.

### 3.3 D1 — materialize each winner once

Keep the first materialize (it is load-bearing: a `MaterializeError::Hard` there is what promotes the next-ranked peer, `keepset.rs:2391-2394`), eliminate the second.

`on_winner` converts the `CanonicalMessage` into a `PreparedWinner` immediately and stores it keyed by item index, dropping the `CanonicalMessage` at once so peak RAM is no worse than today's `prepared`. After `to_keep_set`, the write order still comes from `keep_set.winners`; the prepared entries are looked up, not recomputed.

**Take the conversion by value while you are here (D11).** `on_winner` owns the `CanonicalMessage` and discards it immediately, so `from_canonical_message` gains a by-value form that moves bodies and attach payloads instead of cloning them. This is not a separate optimization to schedule later — doing the move at the same moment removes a full per-winner memcpy *and* the transient double-residency, and doing it any other way means writing the clone twice.

**The review point that must not be waved through:** the two paths merge integrity at different moments. `finalize_with_materialize` sets `msg.fidelity = resolved.items[idx].integrity.clone()` at `keepset.rs:2376`; `prepare_winner` re-merges `entry.integrity` plus the second materialize's fresh soft reasons at `:2646-2656`. Moving the work must produce the *same* `degraded_reasons` set, or the export becomes more optimistic than it is today. DoD-7 asserts this directly, not by inspection.

Expected win: halves per-winner source I/O and body decode. On INC's profile this is the leading candidate for the bulk of the 275 s — but it is a candidate until §3.1 says so.

### 3.4 D2 — O(1) amortized AMap bookkeeping

- `place_and_write_block`: replace the full-vector filter with a `stubbed_upto: usize` watermark. `amap_pages` only ever appends, so only `amap_pages[stubbed_upto..]` can be unwritten.
- `amap_ensure_page`: replace `iter().any()` with a `HashSet<u64>` of registered offsets alongside the vector.

Neither changes placement, offsets, or emitted bytes — the oracle proves it.

**The test asserts complexity, not time** (rule 5): instrument a scan-step counter and assert that total steps stay within a small constant multiple of block count across two fixture sizes (1× and 4×). A superlinear regression fails the test on any machine, at any load.

### 3.5 D3 — one positioned write per block

Two candidates, chosen by measurement:

1. Track `eager.file_pos` and skip the `seek` when the cursor is already at `offset`. The eager path is append-mostly, so most seeks are redundant.
2. `FileExt::seek_write` (Windows) / `write_at` (Unix) to fuse seek and write.

**Explicitly not:** wrapping the eager file in a `BufWriter` while the seeks remain. `BufWriter::seek` flushes, so the buffer would be discarded on essentially every block — a rewrite that measures as noise and reads as an improvement. If buffering is wanted it must come *after* the seeks are gone, and it must handle the AMap stub writes, which land at offsets *behind* the advanced cursor (`lib.rs:349-358`).

### 3.6 D4/D5 — one bounded handle cache

A single `PstHandleCache` owned by the caller, bounded by `--max-open-psts` (default 32, matching the probe path), LRU eviction, shared by the materializer and the attach stream source. This closes D-0074-mat-lru and removes one full NBT+BBT build per source per run.

The borrow conflict at `pst_materializer.rs:490-493` is the real design work: `finalize_with_materialize` holds `&mut` on the materializer across `on_winner`. Resolve with `Rc<RefCell<PstHandleCache>>` (single-threaded path) rather than by keeping two caches. Under `--jobs > 1` each worker owns its own cache — that multiplication is part of §3.8's cost, and it is why parallelism must earn its way in.

### 3.7 D7 — post-write passes

The final hash pass cannot be removed (see D-0070-inline-hash-io in §2.4). What is in scope: measure it, raise the 256 KiB buffer if the measurement supports it, and consider a sequential-access hint on Windows.

**Measure read vs. hash before optimizing either.** The likely answer is that this pass is CPU-bound on the digests, not I/O-bound: MD5 runs around 500 MB/s and SHA-256 around 1–2 GB/s with SHA-NI, so 4 GB costs roughly 8–10 s of pure hashing against ~4 s of sequential read. If that holds, the lever is running the two digests **concurrently over the same buffer** — a win that needs no new dependency and no unsafe mapping.

**Memory-mapping the output temp is declined** — not by rule 4 (which governs sources), but on three independent grounds:

1. **It converts a recoverable error into a process abort.** A read that fails returns `io::Error`. A page fault on a mapped file raises `EXCEPTION_IN_PAGE_ERROR` / `SIGBUS`, which Rust cannot catch — the process dies. Operators do put scratch on network shares. For a tool whose entire 0078 contract is an honest exit code and a quarantined artifact, trading a `Result` for an uncatchable abort at the *last step before rename* is the worst possible place to take that risk, and it violates core mandate 4.
2. **It is a new default production dependency.** `memmap2` is transitive-only today (0.9.10, via the search stack) and already carries this repo's own audit warning (D-0062-audit-warnings). Promoting it to a direct writer dependency needs a measured win under rule 9.
3. **The win is small if the pass is hash-bound.** Zero-copy saves the memcpy, not the hashing — and you still touch every byte.

Recorded here explicitly so this does not get re-proposed as free.

Phase 5 `verify_volumes` is measured and reported as `verify_ms` **before anyone proposes changing it**. It is a fidelity gate, not overhead, and a perf track is exactly the wrong context in which to quietly weaken a verification step. If it turns out to dominate, that is a finding for 0080 QC, not a licence to sample less here.

### 3.8 Parallel materialize — opt-in, and honest about what it costs

`--jobs N` (default **1**). N materialize workers, each with its own handle cache, feeding a **bounded** channel; one writer thread; ordered commit by keep-set index so output order is unchanged. Runtime is dedicated `std::thread` workers over `std::sync::mpsc::sync_channel`, **not** rayon (§1).

#### Source affinity is mandatory, not an optimization

Each worker owning its own bounded handle cache (§3.6) means a worker that touches K distinct sources pays K full NBT+BBT builds (D4). Distribute the queue without regard to source and every worker cycles through every source, evicting and rebuilding constantly — `--jobs 4` would land **slower** than `--jobs 1` while looking like it should be faster.

The good news is that the locality is already there and free: `scan` walks one PST at a time (`scan.rs:535`) pushing candidates as it goes, so items — and therefore `keep_set.winners`, which is item-index order — are **already contiguous by source**. Affinity is achieved by handing each worker a contiguous run, and costs nothing.

Which is precisely why the runtime choice is load-bearing rather than stylistic: a work-stealing scheduler would re-shuffle that contiguity into exactly the pathological interleaving described above. **The distributor must partition by source, and no stage may re-order across source boundaries for load-balancing.** This is a DoD, not a tuning note.

Corollary worth stating plainly: with a small number of large sources, N workers cannot exceed the number of distinct sources without one of them going idle or breaking affinity. `--jobs` is bounded by source count in practice, and the summary should say so rather than let an operator conclude the flag is broken.

The cost that must be stated in the summary, not buried: 0077 attributes per-source CRC counters by sequential snapshot/delta, which is exact only while materialize is sequential (D-0077-parallel-attrib). Under `--jobs > 1` that attribution is no longer sound. The answer is **not** to report per-source numbers anyway:

```
crc_attribution: "per_source" | "aggregate"
```

`--jobs > 1` sets `aggregate`, and per-source CRC fields are omitted rather than filled with a plausible-looking guess. An operator choosing speed is told exactly which evidence they traded for it.

If measurement shows §3.3–§3.6 already meet the target, **`--jobs` should not ship.** Shipping it anyway would trade a 0077 invariant for a win already banked.

### 3.9 D6 — measured and guarded, not redesigned

Report `prepared_bytes_peak` (summed body + buffered-attach bytes retained in `prepared`). Streaming prepare→write behind the same bounded channel is the real fix and is available cheaply once §3.8's plumbing exists — but it is Phase C optional, gated on the measurement, and it must not land alongside the parallel path in one change.

**This is a stability question, not only a speed one, so it does not wait for the redesign.** An export that OOM-panics is worse than a slow one: 0078 gives a cancelled run a quarantined artifact and exit 130, but an allocator abort gives the operator a dead process and a truncated PST at the deliverable path with no summary written. Two things therefore land with the measurement, not after it:

- `prepared_bytes_peak` in the summary, so the ceiling is visible before it is hit.
- A soft warning when it crosses a documented threshold, naming `--jobs`/streaming as the remedy.

The D11 by-value fix in §3.3 also reduces the transient peak, which is the cheapest part of this and lands in Phase 1 regardless.

## 4. Out of scope

- GPU/SIMD hashing; multi-process export.
- Memory-mapping source PSTs (rule 4 — write-lock and network-share corruption risk).
- Any change to MS-PST layout, identity hashing, or the winner ladder (rule 12).
- Rewriting `pst-reader`'s buffering strategy (see D-0079-reader-buffer).
- Weakening Phase 5 verification (§3.7).

### New residuals

| ID | Severity | Item | Notes |
|---|---|---|---|
| D-0079-deterministic-key | — | Derive the store record key from a digest of inputs so unique-pst output is byte-reproducible | Would make `sha256_hex` a meaningful chain-of-custody value and let the oracle compare bytes. Changes every produced PST's `PidTagRecordKey` and folder EntryIDs, so it is a **product** decision, not a perf track's call. See D10. |
| D-0079-reader-buffer | P3 | `PstFile` holds one 64 KiB `BufReader` (`pst-reader/src/lib.rs:105`); random block reads defeat it | A seek discards the buffer, so a ~8 KiB block read can cost a 64 KiB refill. Suspected read amplification; belongs to a `pst-reader` track with its own fixtures, not to unique-pst. Measure here, fix there. |
| D-0079-stream-prepare | — | Pipeline prepare→write so RAM is bounded by in-flight winners, not by winner count | §3.9. Cheap once §3.8 plumbing exists. Note the exposure is **bodies** (`body_plain`/`body_html`, uncapped) and the *aggregate* of ≤64 KiB attach buffers — not large attachments, which already stream. 0079 ships the measurement + threshold warning; this residual is the structural fix. |
| D-0079-operator-multigb | — | Operator-local multi-GB before/after with the §3.2 oracle | Carries D-0070-operator-multigb. Cannot be CI. |

## 5. Preconditions

- 0078 merged (`9c8be49`) — `PhaseTimings` extends the summary contract 0078 defined.
- No open ledger transaction on the export path.
- A fixture large enough that the 1×/4× complexity assertion in §3.4 is meaningful.

## 6. Risks

| Risk | Mitigation |
|---|---|
| A "safe" refactor silently changes the export | §3.2 oracle is built first and gates every phase (rule 2) |
| D1 fix changes `degraded_reasons` via the merge-point shift | DoD-7 asserts the reason set explicitly; not eyeballed |
| Wall-clock tests flake in CI and get disabled | Complexity is asserted by operation count (rule 5) |
| Parallel materialize silently corrupts per-source CRC attribution | `crc_attribution: aggregate` + omitted fields (§3.8) |
| Parallelism ships because it was built, not because it was needed | §3.8 last paragraph: if §3.3–3.6 hit the target, do not ship it |
| Unbounded queue turns a speed fix into an OOM | Rule 7, bounded channel only |
| Handle cache eviction thrashes on many sources | LRU default 32 matches the proven probe path; `--max-open-psts` measured |
| A work-stealing runtime shreds the free source locality and makes `--jobs 4` slower than `--jobs 1` | Dedicated threads + source-affinity partitioning as a DoD, not a tuning note (§3.8); rayon recommended against with reason (§1) |
| OOM on a large run — worse than slow, because it defeats 0078's quarantine + exit contract | `prepared_bytes_peak` + threshold warning ship with the measurement (§3.9); D11 by-value cut lands in Phase 1 |
| mmap re-proposed later as a free win for the hash pass | Declined in §3.7 with the abort/dependency/CPU-bound reasoning recorded |
| Optimization erodes cancellation responsiveness | Rule 11; 0078 exit-130 + quarantine tests stay green |
| Fixture-scale wins do not transfer to multi-GB | D2 is proven by complexity, not by fixture time; operator run is D-0079-operator-multigb |

## 7. Definition of Done

1. `PhaseTimings` in `summary.json` on every run, including cancelled runs, with `unaccounted_ms` computed.
2. `source_pst_opens`, `messages_materialized`, `bytes_written_total` reported.
3. Export equivalence oracle (§3.2) exists as a test helper and passes baseline-vs-baseline before any optimization.
4. `baseline.md` records per-phase timings for the fixture set **before** any change.
5. Every winner is materialized exactly once: `messages_materialized == keep_set.stats.unique`, asserted in an integration test.
6. `prepare_winner`'s second `materialize` call is gone.
6a. `from_canonical_message` has a by-value form that moves bodies and attach payloads; no per-winner deep clone remains on the hot path (D11).
6b. `prepared_bytes_peak` reported, with a soft warning above a documented threshold.
7. **Reason-set equivalence:** for a fixture with attach failures, CRC-suspect messages, and a promoted winner, `degraded_reasons` per winner is identical before and after §3.3.
8. AMap scan steps are O(1) amortized per block, asserted by operation count across 1× and 4× fixtures.
9. Block writes issue one positioned write per block; no `BufWriter` wraps a seeking writer.
10. One bounded LRU `PstHandleCache` shared by materializer and attach stream source; `--max-open-psts` honored; D-0074-mat-lru closed in `deferred.md`.
11. `verify_ms` and the final-hash cost are reported; Phase 5 verification is **not** weakened.
12. If `--jobs` ships: default 1, bounded channel, single writer, ordered commit, and `crc_attribution: "aggregate"` with per-source CRC fields omitted when `N > 1`.
12a. If `--jobs` ships: **source-affinity partitioning asserted by test** — each worker receives a contiguous single-source run, and a multi-source fixture proves `source_pst_opens` does not scale with N. No work-stealing runtime.
13. If `--jobs` does not ship, `review.md` states why, with the numbers that made it unnecessary.
14. Measured speedup documented in `review.md` per phase, with the fixture and machine named.
15. Zero fidelity regression: full 0071/0073/0074/0077/0078 test suites green, including exit-code and quarantine tests.
16. Cancellation latency measured before and after; no regression.
17. No new default dependency without a recorded number justifying it.
18. `deferred.md` updated: D-0074-mat-lru closed; D-0070-inline-hash-io narrowed with the finalize-seek reason; D-0073-vec-events marked declined-with-reason; D-0079-* added.
19. `conductor.md` + `sequencing.md` rows updated; `review.md` written.
20. `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`.

## 8. Verification

1. Oracle self-test: two baseline runs over the same fixture compare equal (proves the oracle tolerates D10's volatile fields and nothing more).
2. Baseline phase timings captured and committed to `baseline.md`.
3. After §3.3: oracle passes; `messages_materialized == unique`; reason-set equivalence on the degraded fixture.
4. After §3.4: operation-count assertion across 1×/4×; oracle passes.
5. After §3.5: oracle passes; syscall shape confirmed by code review plus timing delta.
6. After §3.6: `source_pst_opens` drops by the expected count; oracle passes; eviction exercised with more sources than the cap.
7. Cancel mid-write still yields exit 130, quarantined artifact, and `artifact_state` per 0078.
8. Full workspace gate.
9. Operator-local multi-GB run recorded in `review.md` if available; explicitly marked absent if not.

## 9. Handoff

**Do:** build the oracle first; record `baseline.md` before touching anything; fix complexity before constants; report timings even when nobody is watching; state the parallel-materialize trade in the summary, not in a comment.

**Do not:** claim a speedup without a phase number; wrap a seeking writer in a `BufWriter`; memory-map a source PST *or* the output temp (§3.7); let any queue be unbounded; use a work-stealing runtime for materialize or let any stage re-order across source boundaries; ship `--jobs` because it was built; fill per-source CRC fields under parallel materialize; weaken Phase 5 verification to make a number look better; change identity, the winner ladder, or write order; treat "byte-identical output" as an available test (D10); assume large attachments are the RAM risk — they already stream (§2.2 D6).
