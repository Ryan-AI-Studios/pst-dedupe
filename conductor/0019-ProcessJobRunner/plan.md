# 0019 — In-app process job runner — Plan

Phased checklist. Map phases to DoD items in `spec.md` §7. Execute in `C:\dev\dedupe`.

> **Ledger:**  
> `ledgerful ledger start 0019-processjobrunner --category FEATURE --message "In-app process job runner: pool, cancel, progress"`  
> Commit in Finalize.

---

## Phase 0 — Preconditions → DoD-10 baseline

- [x] Confirm **0015** jobs/checkpoints API; read `matter-core` `JobState` transitions
- [x] Confirm **0016** / **0018** Completed: cancel → **Paused**; they **create their own jobs** today
- [x] Read plan-of-record §4.6 (Tokio trap)
- [x] Accept design locks from review: **watch not crossbeam MPMC**, **Option C job inject**, **single matter worker**, **Drop join**
- [x] `cargo test -p matter-core` / `extract-pst` / `ingest-purview` green

## Phase 1 — Design lock → DoD-2/3/4/5/6 prep

- [x] Freeze crate name: `process-runner`
- [x] Freeze concurrency: **one Running job per matter** — **reject** second start
- [x] Freeze Matter affinity: **single worker thread owns Matter** — **forbid** `Arc<Mutex<Matter>>` + rayon for P0
- [x] Freeze progress: **`tokio::sync::watch`** latest snapshot (sync use OK); optional `broadcast` for full event log; **no** crossbeam multi-sub bus
- [x] Freeze cancel: `Arc<AtomicBool>`; cooperative; cancel → stage **Paused**
- [x] Freeze Drop/shutdown: set cancel + **join** worker
- [x] Freeze job authority: **runner only** creates job; handlers receive `job_id` (**Option C mandatory**)
- [x] Design `JobHandler` + `JobContext` + `JobParams` JSON:
  - [x] ingest: `{ "path": "…" }`
  - [x] extract_pst: `{ "source_id", "pst_item_id" }` or path form
- [x] Sketch CLI or example smoke

## Phase 2 — Stage API inject (0016/0018) → DoD-5 **(blocking for real handlers)**

- [x] `ingest-purview`: `ingest_path_on_job(matter, path, limits, job_id, cancel)` — **no** internal `create_job`
- [x] `extract-pst`: `extract_pst_item_on_job` / path `_on_job` — **no** internal `create_job`
- [x] Existing APIs = `create_job` + `set Running` + call `*_on_job` (back-compat)
- [x] `resume_*` unchanged (already take `job_id`)
- [x] Tests: both crates green; unit/assert on_job does not insert second job

## Phase 3 — matter-core helpers (optional) → DoD-3 prep

- [x] Optional: `list_jobs` / by-state for UI
- [x] Prefer **no** schema bump unless params must survive process kill beyond existing checkpoints

## Phase 4 — Scaffold `process-runner` → DoD-1, DoD-2

- [x] `cargo new --lib crates/process-runner`
- [x] Workspace member + deps (`matter-core`, `tokio` with **`sync` only** if possible for watch/broadcast — avoid pulling full rt if not needed)
- [x] Modules: `lib.rs`, `error.rs`, `cancel.rs`, `progress.rs`, `handler.rs`, `runner.rs`, `config.rs`
- [x] Features: `ingest`, `extract_pst` (default on)
- [x] README: worker model, watch, Option C, Drop join, no UI-thread extract
- [x] `cargo check -p process-runner`

## Phase 5 — Runner core → DoD-2, DoD-3, DoD-4, DoD-6, DoD-7

- [x] `CancelToken` (`Arc<AtomicBool>`)
- [x] Progress: `watch::Sender<JobProgressSnapshot>` (+ optional broadcast)
- [x] Handler registry
- [x] **Single** matter worker thread + command queue (start/resume/cancel/shutdown)
- [x] `start`: `create_job` → Running → run handler with that `job_id`
- [x] `resume` / `cancel` / `shutdown` + **Drop = cancel+join**
- [x] Single-flight reject
- [x] Unit tests: mock handler, cancel, single job_id, Drop joins

## Phase 6 — Real handlers → DoD-5, DoD-8

- [x] `IngestHandler` → `*_on_job` only
- [x] `ExtractPstHandler` → `*_on_job` only
- [x] Update watch on start / checkpoint poll or summary / terminal
- [x] Integration: fixture extract; cancel + resume; assert **one** job row
- [x] Integration: unknown kind / double start

## Phase 7 — Smoke surface + docs → DoD-9

- [x] CLI or `examples/run_extract.rs` (print watch snapshots) — `examples/run_job.rs`
- [x] ARCHITECTURE + root README
- [x] extract-pst / ingest-purview README: on_job + runner link
- [x] Note for **0020**: `watch_progress` + start/cancel only on UI thread

## Phase 8 — Verification → DoD-10

- [x] `cargo test -p process-runner`
- [x] `cargo test -p extract-pst` / `ingest-purview` / `matter-core`
- [x] `cargo fmt --all --check`
- [x] `cargo clippy` on touched crates `-D warnings` (workspace clippy deferred to orchestrator if needed)
- [ ] `cargo test --workspace` (optional if time)
- [ ] `ledgerful verify` (**required** — orchestrator / finalize)
- [ ] Capture evidence for `review.md`

## Phase 9 — Finalize → DoD-11

- [ ] Write `review.md` (watch, Option C, worker model, Drop join, deferred multi-job)
- [ ] Update `../conductor.md`: **0019** → **Completed**
- [ ] Update `../sequencing.md`
- [ ] Commit ledger TX
- [ ] Handoff: **0020** consumes watch; **0021** registers handler later

---

## Suggested file map

```
crates/process-runner/
  Cargo.toml
  README.md
  src/
    lib.rs
    error.rs
    cancel.rs          # Arc<AtomicBool>
    progress.rs        # watch (+ optional broadcast)
    handler.rs
    runner.rs          # single matter worker + Drop join
    config.rs
    handlers/
      mod.rs
      ingest.rs
      extract_pst.rs
  tests/
    integration.rs

crates/ingest-purview/   # *_on_job inject
crates/extract-pst/      # *_on_job inject
```

---

## Default config

| Setting | Default |
|---|---|
| Worker model | **1 matter thread** (owns `Matter`) |
| Progress | **`watch` latest snapshot** |
| Optional event log | `broadcast` (if enabled) |
| Cancel | `Arc<AtomicBool>` → stage **Paused** |
| Second start | **Reject** busy |
| Drop | cancel + **join** worker |

---

## Handoff notes

- Stages keep checkpoint grain; runner does not reimplement expand/extract.
- **Runner creates the only job row** for orchestrated runs (Option C).
- **0020** uses `watch_progress` — do not use crossbeam multi-sub for UI.
- **0021** adds a handler; no runner rewrite.
- Never run extract/ingest on the egui thread.
- Never share `Matter` across a rayon pool under a mutex for P0.
- Single-exe / no-daemon invariant unchanged.
