# 0019 — In-app process job runner (no daemons)

- **Track ID:** 0019-ProcessJobRunner
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → §4.5–4.6 (lifecycle + Tokio trap + resumable jobs), Series A / **019**, §17 (`tokio` sync primitives; rayon only for future non-Matter CPU stages)
- **Cross-repo contract:** n/a
- **Status:** Ready — not started
- **Depends on:** **0015-MatterStore** (Completed — jobs/checkpoints). **Recommended wire-ups:** **0016** (`ingest-purview`), **0018** (`extract-pst`) — both Completed.

---

## 1. Objective

Own **in-process** background work for Dedupe Desk:

1. A **single-process job runner** (no user-started daemons, Redis, Postgres, or Docker).
2. A **CPU/IO blocking pool** boundary so extract/ingest never run on the GUI thread or Tokio async workers (plan §4.6).
3. **Cancel** + **progress events** to UI/CLI consumers.
4. **Resume** orchestration that reuses durable `matter-core` jobs/checkpoints and existing stage APIs (`resume_ingest`, `resume_extract`).
5. A small **pluggable handler** surface so later tracks (0021 dedupe, 0029 index, …) register without forking the runner.

This track **orchestrates** work; it does **not** reimplement ZIP expand, PST parse, or logical hash.

---

## 2. Context (read before starting)

### 2.1 Plan-of-record

- `C:\dev\Dedupe-plan.md` §4.6: UI/orchestration may be async; extractors/hash/OCR only via `spawn_blocking` and/or **rayon**; cancel tokens; progress channels; batch SQLite commits (already in stage crates).
- Job state machine (already in `matter-core::JobState`):  
  `Pending → Running → (Paused|Failed|Cancelled) → Running → Succeeded` (+ retry from Failed/Cancelled → Pending/Running).
- Guardrails: `../TRACK-GUARDRAILS.md`.
- Sequencing: unblocks **0020** Desk shell; with **0018** unblocks **0021** MatterDedupeJob.

### 2.2 What already exists (do not reimplement)

| Layer | Status | 0019 role |
|---|---|---|
| `matter-core` jobs | `create_job`, `set_job_state`, `put_checkpoint`, `get_checkpoint`, transition rules | Persistence of truth |
| `ingest-purview` | `ingest_path` / `resume_ingest`, cancel callback, stage `expand` | **Handler** for kind `ingest` |
| `extract-pst` | `extract_pst_item` / `resume_extract` / `extract_pst_path`, cancel, stage `pst_extract` | **Handler** for kind `extract_pst` |
| Stage checkpoints | Leaf expand (0016); mid-folder extract (0018) | Runner only **loads/forwards** resume |

### 2.3 What 0018 explicitly deferred to 0019

From `../0018-PstExtractorAdapter/review.md` **D-0018-04**: process runner / progress channels.

`extract-pst` and `ingest-purview` both document: **call from blocking worker only** — 0019 must enforce that for Desk/CLI orchestration.

### 2.4 Product / desktop rules

- Single-exe Desk; **no** external job daemon.
- Child processes only if a future optional plugin requires them (OCR etc.) — **out of scope** for default runner path.
- AI off by default.
- Never mutate source Purview/PST evidence (handlers already enforce).

---

## 3. In scope

### 3.1 Crate / workspace

1. New library crate **`crates/process-runner`** (name flexible; plan §4.4 “process-pipeline” is broader — keep this crate **runner-only**, not OCR/hash logic).
2. Workspace member; deps (plan §17, pin carefully):
   - `matter-core` (path)
   - `std::sync::atomic` + `Arc` for cancel (no exotic cancel crate required)
   - **Progress (required model — see §3.2.2):** prefer **`tokio::sync::watch`** for UI “latest snapshot” progress (usable **without** a Tokio runtime — call `borrow`/`send` synchronously). Optional `tokio::sync::broadcast` only if multiple consumers each need a full event stream.
   - **Do not** use `crossbeam-channel` as the multi-subscriber progress bus (MPMC consumers **steal** messages from each other — see §3.2.2).
   - **Do not** default to a multi-worker **rayon** pool owning `Matter` (see §6.1).
   - Optional `tokio` `rt-*` only if Desk later needs async orchestration; **P0 runner core stays sync** (`std::thread` worker).
3. Path deps / optional features for handlers:
   - feature `ingest` → `ingest-purview`
   - feature `extract_pst` → `extract-pst`
   - default features enable both for Desk builds

### 3.2 Core abstractions

#### 3.2.1 Cancel (cooperative)

```rust
// Required shape — Arc<AtomicBool>, cloneable into handlers
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}
impl CancelToken {
    pub fn new() -> Self;
    pub fn cancel(&self);           // store(true, Ordering::SeqCst or AcqRel)
    pub fn is_cancelled(&self) -> bool;
    pub fn as_fn(&self) -> impl Fn() -> bool + '_; // for 0016/0018 cancel: Option<&dyn Fn() -> bool>
}
```

**Rules:**

1. Cancellation is **cooperative** only — no forced thread kill.
2. Handlers (0016/0018) must poll between units (message / zip entry) — already designed that way.
3. UI “Cancel” → `CancelToken::cancel()`; stages set job **Paused** (not silent drop).
4. **`Drop` / `shutdown` on `ProcessRunner`:** set cancel flag **and** **join** the matter worker thread (with timeout policy documented) so an in-flight SQLite batch can finish or cleanly pause — reduce mid-commit process exit risk. Do not detach the worker on app close without join/cancel.

#### 3.2.2 Progress: watch (latest) first — not crossbeam MPMC

**Problem:** `crossbeam-channel` is **MPMC**, not broadcast. Two subscribers (UI + logger) will **partition** events (UI gets #1, logger #2, …). That is wrong for progress.

**Required progress design:**

| Channel | Role | When |
|---|---|---|
| **`watch` (primary)** | Holds the **latest** `JobProgressSnapshot` only | **UI progress bars** — if UI is busy painting, worker never blocks on a full queue; reader always sees freshest state |
| **`broadcast` (optional)** | Every subscriber gets every event | CLI logger / tests that need a full audit of events; lagging consumers may lag/skip per tokio broadcast rules — document |
| **crossbeam MPMC** | **Forbidden** as the multi-subscriber progress bus | Steal semantics |

`tokio::sync::watch` / `broadcast` may be used **synchronously** (`send`, `borrow`, `try_recv`) without spawning a Tokio runtime.

Snapshot / event fields (minimum):

| Field | Notes |
|---|---|
| `job_id`, `kind`, `matter_id` | Identity |
| `state` | pending/running/paused/… |
| `stage` | e.g. `expand`, `pst_extract` |
| `completed_count` | from checkpoint / summary |
| `total_hint` | optional |
| `message` | optional human string |
| `error_summary` | on fail |
| `updated_at` | ts |

**P0 quality bar:**

- Watch always updated on Started, every meaningful batch/checkpoint (or poller), and terminal state.
- Optional: runner-side poll of `get_checkpoint` while handler blocks, writing into watch so UI moves during long extracts without stage API changes.

#### 3.2.3 Job handler trait

```rust
pub trait JobHandler: Send + Sync {
    fn kind(&self) -> &'static str; // "ingest" | "extract_pst" | future "dedupe" …
    /// Run or resume. Must honor cancel and may block (called on matter worker thread).
    /// `ctx.job_id` is always pre-created by the runner — never create a second job.
    fn run(&self, ctx: &JobContext) -> Result<JobOutcome, JobError>;
}

pub struct JobContext<'a> {
    pub matter: &'a Matter, // owned by the single matter worker thread
    pub job_id: &'a str,    // runner-created; required
    pub source_id: Option<&'a str>,
    pub params_json: &'a str,
    pub cancel: &'a CancelToken,
    pub progress: ProgressSink, // updates watch (+ optional broadcast)
    pub is_resume: bool,
}
```

Handlers wrap existing crates via **job-id injection only** (§3.4):

| Kind | Start (on existing job) | Resume |
|---|---|---|
| `ingest` | `ingest_path_on_job` / equivalent | `resume_ingest` |
| `extract_pst` | `extract_pst_item_on_job` / path variant | `resume_extract` |

### 3.3 Runner API

```rust
pub struct ProcessRunner { /* matter worker thread, handlers, cancel, watch tx */ }

impl ProcessRunner {
    pub fn new(config: RunnerConfig) -> Self;
    pub fn register(&mut self, handler: Arc<dyn JobHandler>);
    /// create_job + set Running + queue work on matter worker. Returns job_id.
    pub fn start(&self, matter_root: &Utf8Path, kind: &str, params: JobParams) -> Result<String>;
    pub fn resume(&self, matter_root: &Utf8Path, job_id: &str) -> Result<()>;
    pub fn cancel(&self, job_id: &str) -> Result<()>;
    /// UI: subscribe to latest snapshot (watch receiver).
    pub fn watch_progress(&self) -> watch::Receiver<JobProgressSnapshot>;
    /// Optional full event stream (broadcast).
    pub fn subscribe_events(&self) -> broadcast::Receiver<ProgressEvent>;
    pub fn active_job(&self, matter_id: &str) -> Option<JobSnapshot>;
    /// cancel + join worker; safe app shutdown
    pub fn shutdown(&self);
}

impl Drop for ProcessRunner {
    // cancel + join worker (same as shutdown); never silent detach
}
```

**Concurrency policy (P0 — mandatory):**

| Policy | Choice |
|---|---|
| Per matter | **At most one heavy job Running** (reject second start with busy error) |
| Execution | **Exactly one matter worker thread** owns `Matter` for the active job — **not** a rayon pool of Matter users |
| Shutdown | `cancel` + **join** worker; document join timeout if any |

### 3.4 Matter integration — job-id injection **mandatory** (Option C)

**Problem:** `ingest_path` and `extract_pst_item` currently **create their own job** rows. Wrapping them as black boxes creates **double jobs** and broken resume authority.

**Required architecture (Option C — only defensible P0 path):**

1. **Runner is the sole authority** that:
   - `Matter::create_job(kind)`
   - `set_job_state(Running)`
   - passes `job_id` into the blocking handler
2. Stage crates expose thin **on-job** entry points that **do not** create jobs:
   - `ingest_path_on_job(matter, path, limits, job_id, cancel)`
   - `extract_pst_item_on_job(matter, source_id, pst_item_id, limits, job_id, cancel)`
   - path variants as needed
3. Existing public APIs become thin wrappers: create job → call `*_on_job` (back-compat for tests/CLI that don’t use the runner yet).
4. Resume continues to use `resume_ingest` / `resume_extract` with the **same** `job_id`.

**Forbidden:** Option A (outer session job + inner stage job). Do not leave double-job as “temporary.”

This requires small, intentional patches to **0016** / **0018** crates as part of **0019** DoD.

### 3.5 CLI / smoke surface (lightweight)

Optional but strongly recommended for DoD without GUI:

- Extend `pst-dedup-cli` with e.g. `job run ingest|extract …`, `job resume`, `job cancel`, printing progress lines on stderr.
- Or a binary example under `process-runner` examples.

Full Desk UI is **0020**.

### 3.6 Tests (required)

1. **Start + succeed:** mock or fixture handler completes → **exactly one** job row `Succeeded`; watch shows terminal snapshot.
2. **Cancel mid-run:** cancel → stages **Paused**; watch reflects paused; worker cooperative exit.
3. **Resume:** after pause, `resume` continues without resetting checkpoints / without second job id.
4. **Single job row:** assert `create_job` count / job_id identity for inject path (no double job).
5. **Blocking boundary:** handler runs on matter worker thread, not caller.
6. **Single-flight:** second `start` while Running → reject.
7. **Watch semantics:** rapid progress updates; subscriber only needs latest (no MPMC steal test — document watch).
8. **Shutdown/Drop:** cancel + join does not leave worker detached (unit/integration).
9. **Handler registry:** unknown kind → structured error.
10. **Integration (preferred):** fixture PST under runner + cancel/resume.
11. Workspace gate + **`ledgerful verify`**.

### 3.7 Docs

- `crates/process-runner/README.md`: matter worker model, cancel/Drop, **watch vs broadcast**, job-id authority, **never call extract/ingest on UI thread**.
- ARCHITECTURE / root README crate map.
- Update 0016/0018 README: runner owns job creation; link `*_on_job` APIs.
- `review.md` on completion.

### 3.8 Optional (not DoD)

- Full tokio **runtime** for egui (watch works without it).
- Multi-job parallel stages inside one matter.
- Rayon for pure CPU stages that **do not** hold `Matter` (future).
- Distributed/workers (Series I).
- OCR/AI plugin process isolation.
- Job priority queue UI.

---

## 4. Out of scope (do NOT do here)

| Deferred | Work |
|---|---|
| **0016/0018** (done) | Expand / extract algorithms (only thin job-id injection) |
| **0020** | Desk shell UX (consume runner) |
| **0021** | Dedupe job handler implementation (may register stub/trait only) |
| **0029** | Tantivy index job |
| **0044** | Workflow engine / multi-step graphs |
| **0058+** | Multi-user remote workers |
| — | External systemd/Windows services as required path |
| — | Mutating source evidence |

---

## 5. Preconditions & dependencies

- **P1 (blocking):** **0015** — `Job` / `JobState` / checkpoints.
- **P2 (recommended for real handlers):** **0016**, **0018** Completed.
- **P3:** Plan §4.6 accepted.
- *Verified from 0018 review:*
  - `extract_pst_item` / `resume_extract` + cancel → Paused
  - Mid-folder checkpoints; blocking contract documented
  - D-0018-04 deferred progress to 0019
- *Verified from matter-core:*
  - Job transitions include Paused↔Running, Failed retry paths

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| **Double job rows** | **Option C mandatory:** runner creates job; `*_on_job` only; test one job_id |
| GUI freeze | Handlers only on matter worker thread; README |
| **MPMC progress steal** | **watch (latest) primary; no crossbeam multi-sub bus** |
| Progress storm blocking worker | watch overwrites latest; never unbounded queue of 10k UI frames |
| Cancel races / mid-commit exit | `Arc<AtomicBool>`; stages poll; **Drop joins** worker |
| Matter `!Sync` / lock contention | **Single matter worker only** — no `Arc<Mutex<Matter>>` across rayon |
| Scope creep into workflow engine | Single-kind jobs only; graphs → 0044 |
| Tokio trap | No handler on async executor; optional tokio sync primitives only |

### 6.1 SQLite / Matter handle constraint (critical)

`rusqlite::Connection` is **not `Sync`**.

| Approach | P0 status |
|---|---|
| **A. Single worker thread owns `Matter`** | **Required** — sequential jobs, no pool contention on 50GB extract commits |
| **B. Open Matter per job on many pool threads** | **Forbidden for P0** heavy path |
| **C. `Arc<Mutex<Matter>>` + rayon** | **Forbidden for P0** — batch commits will thrash the lock and starve workers |

Optional future: pure CPU stages that never touch SQLite may use rayon **after** data is loaded — not the default extract/ingest path.

Do not require `Matter: Sync`.

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Crate:** `crates/process-runner` workspace member; tests run.
- [ ] **DoD-2 — Matter worker boundary:** Handlers run only on the **single matter-owning worker thread** (not UI, not async executor, not rayon+Mutex Matter).
- [ ] **DoD-3 — Lifecycle:** runner `create_job` → Running → Succeeded; cancel → **Paused** (stage-aligned); resume continues.
- [ ] **DoD-4 — Progress:** **watch** latest snapshot updated for start / mid / terminal; no crossbeam multi-sub steal; optional broadcast documented if present.
- [ ] **DoD-5 — Job-id injection (Option C):** `*_on_job` in ingest-purview + extract-pst; **one job row** per run; runner is sole job creator for orchestrated runs.
- [ ] **DoD-6 — Cancel token:** `Arc<AtomicBool>`; stages polled; **Drop/shutdown cancel+join**.
- [ ] **DoD-7 — Single-flight / errors:** Second start rejected; unknown kind fails cleanly.
- [ ] **DoD-8 — Tests:** Cancel + resume + single job_id + shutdown join; preferred real PST integration.
- [ ] **DoD-9 — Docs:** process-runner README (watch, Option C, worker model) + ARCHITECTURE/README.
- [ ] **DoD-10 — Workspace gate:** fmt, clippy `-D warnings`, tests, **`ledgerful verify`**.
- [ ] **DoD-11 — Recorded:** `review.md`; `../conductor.md` → **Completed**; ledger TX (`FEATURE`).

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p process-runner
cargo test -p matter-core
cargo test -p extract-pst
cargo test -p ingest-purview
cargo test --workspace
ledgerful verify
```

---

## 9. Acceptance narrative

An operator (or test) can:

1. Open a matter with a discovered PST (0016) and/or fixture.
2. `ProcessRunner::start(extract_pst, …)` creates **one** job row and runs work on the **matter worker thread**.
3. UI reads **latest** progress via **watch** (never blocks the worker on a full queue); cancel → **Paused** + durable checkpoint.
4. `resume(job_id)` finishes extract without duplicate items and without a second job id.
5. Same pattern for package `ingest` via `*_on_job`.
6. App shutdown: runner Drop/cancel **joins** the worker.
7. **0020** only watches progress + start/cancel; never calls extract on the UI thread.
8. **0021** registers a `dedupe` handler without rewriting the runner.
