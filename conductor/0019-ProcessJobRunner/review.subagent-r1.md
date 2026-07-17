# 0019 Internal Review R1

## Verdict: NEEDS_FIX

## Scope Reviewed

| Area | Paths |
|---|---|
| Spec / plan | `conductor/0019-ProcessJobRunner/spec.md`, `plan.md` |
| Runner crate | `crates/process-runner/**` |
| Option C inject | `crates/ingest-purview/src/ingest.rs`, `crates/extract-pst/src/extract.rs` |
| Stage tests | `crates/ingest-purview/tests/integration.rs`, `crates/extract-pst/tests/integration.rs` |
| Docs | `crates/process-runner/README.md`, root `README.md`, `ARCHITECTURE.md`, stage READMEs |
| Completeness sweep | `TODO`/`FIXME`/`todo!`/`unimplemented!`/`placeholder`/`stub` under `process-runner` (none) |

Read-only review of implementation on branch `feat/0019-process-job-runner`. Gates (`cargo test`, clippy, `ledgerful verify`) were **not** executed in this pass — matrix marks them Not verifiable.

---

## DoD Matrix

| DoD | Status | Evidence | Tests | Gap |
|---|---|---|---|---|
| **DoD-1 — Crate** | Met | `crates/process-runner` in workspace `Cargo.toml`; modules match plan | Integration suite present | Gates not run here |
| **DoD-2 — Matter worker boundary** | Met | Single `matter-worker` thread; `Matter::open` only inside worker; handlers `Arc<dyn JobHandler>` run there; no rayon/`Arc<Mutex<Matter>>` for jobs | `handler_runs_on_worker_not_caller` asserts thread name `matter-worker` | — |
| **DoD-3 — Lifecycle** | Partial | `run_start`: `create_job` → Running → handler → `finalize_job`; cancel maps stage cancel → Paused; `resume` same `job_id` | Mock: succeed / cancel→Paused / resume same id; real ingest succeed | Early `start`/`resume` accept errors mis-reported (Finding P1); real cancel→resume not proven via runner |
| **DoD-4 — Progress** | Partial | `tokio::sync::watch` primary; optional broadcast; no crossbeam; start + terminal snapshots published | Terminal watch test; ingest asserts terminal snap | **No mid-run progress** from real handlers or runner poller (Finding P2) |
| **DoD-5 — Option C** | Met | Handlers call only `*_on_job` / `resume_*`; stage wrappers still create job for back-compat; on_job tests assert single row | Stage on_job tests + runner ingest/extract one-job tests | — |
| **DoD-6 — Cancel + Drop join** | Met | `CancelToken` = `Arc<AtomicBool>`; `cancel` sets flag; stages poll; `shutdown`/`Drop` cancel + join | `cancel_mid_run_pauses`; `drop_joins_without_hang` | Join timeout policy only in code comment, not README (P3) |
| **DoD-7 — Single-flight / errors** | Partial | `Busy` on second start; `UnknownKind` on unknown kind | Both tested | Missing-job / early matter errors become `WorkerGone` (Finding P1) |
| **DoD-8 — Tests** | Partial | Mock cancel, resume, single job_id, shutdown join, busy, unknown kind, thread boundary, ingest zip, extract smoke | See tests column | Real PST test weak; no runner cancel+resume on real handlers (Finding P2) |
| **DoD-9 — Docs** | Met | process-runner README covers worker, watch, Option C, Drop join, no UI-thread extract; ARCHITECTURE + root README + stage READMEs updated | — | Join timeout policy not in README (P3) |
| **DoD-10 — Workspace gate** | Not verifiable | Plan: clippy partial; `ledgerful verify` open | — | Must be run/recorded before close |
| **DoD-11 — Recorded** | Unmet | No `review.md`; conductor still **Ready**; ledger finalize open | — | Finalize phase (expected pre-close) |

### Design locks (§3)

| Lock | Status | Notes |
|---|---|---|
| Option C sole job creator | Met | Runner creates; handlers never `create_job` |
| Single matter worker | Met | One named thread; sequential command loop |
| watch not crossbeam | Met | `tokio::sync::watch` + optional broadcast only |
| Drop join | Met | `Drop` → `shutdown` → cancel + join |
| Single-flight | Met | Accept lock + `active` Busy reject (global = one worker) |
| cancel → Paused | Met | Stages set Paused; handlers map `cancelled` → `JobOutcome::Paused`; finalize trusts durable |

### Risks (§6)

| Risk | Mitigated? |
|---|---|
| Double job rows | Yes — Option C + tests |
| GUI freeze | Yes — worker boundary + docs |
| MPMC steal | Yes — watch only |
| Progress storm | Yes — watch overwrite |
| Cancel / mid-commit exit | Mostly — cooperative + join; no hang timeout policy documented |
| Matter !Sync | Yes — single worker owns Matter per job |
| Tokio trap | Yes — `tokio` features `sync` only; no handler on async executor |

---

## Findings

### [P1] Early `start`/`resume` errors drop the reply channel → caller sees `WorkerGone`

**Confidence:** High

**Requirement:** DoD-3 lifecycle errors; DoD-7 structured failures; resume by `job_id`

**Location:**
- `C:\dev\Dedupe\crates\process-runner\src\runner.rs` — `run_start` (`create_job` / `set_job_state` use `?` before `reply.send`)
- `C:\dev\Dedupe\crates\process-runner\src\runner.rs` — `run_resume` (`get_job` maps to `JobNotFound` then `?` **without** `reply.send`)
- `C:\dev\Dedupe\crates\process-runner\src\runner.rs` — worker arms discard `Err` from `run_start`/`run_resume` (“reply may already have been sent”)
- Call sites: `start` / `resume` map disconnected reply → `RunnerError::WorkerGone`

**Problem:** On accept-path failures *before* the success reply, the `Sender` is dropped without sending a typed error. Callers receive `WorkerGone` (channel closed), not `JobNotFound` / `Matter` / etc.

**Evidence:**
```rust
// run_resume — JobNotFound never reaches the caller typed:
let job = matter
    .get_job(job_id)
    .map_err(|_| RunnerError::JobNotFound(job_id.to_string()))?;
// no reply.send(Err(...)) before ?

// worker:
Err(e) => { let _ = e; }  // drops reply → start/resume recv → WorkerGone
```
No test covers `resume` of a missing `job_id` or pre-reply `create_job` failure.

**Failure scenario:** UI/CLI calls `resume(matter, "typo-id")` → `WorkerGone` → consumer assumes runner died, may tear down or restart incorrectly; real `JobNotFound` is unusable.

**Correction:** On every early exit in `run_start`/`run_resume`, `reply.send(Err(...))` before return (or use a scope guard / always-reply helper). Worker should not rely on drop for error delivery. Add tests: resume missing id → `JobNotFound`; optional create_job failure → non-`WorkerGone` error.

**Verification:** `cargo test -p process-runner` with new error-path tests.

**Deferrable:** No

---

### [P2] DoD-4 mid progress not implemented for production handlers

**Confidence:** High

**Requirement:** DoD-4 — watch updated for **start / mid / terminal**; §3.2.2 P0 bar (“every meaningful batch/checkpoint (or poller)”)

**Location:**
- `C:\dev\Dedupe\crates\process-runner\src\handlers\ingest.rs` — single `progress.patch` at entry (stage/message only)
- `C:\dev\Dedupe\crates\process-runner\src\handlers\extract_pst.rs` — same
- `C:\dev\Dedupe\crates\process-runner\src\runner.rs` — no checkpoint poller while handler blocks
- Stages do write checkpoints (`expand` / `pst_extract`) but runner never surfaces `completed_count` mid-run

**Problem:** During long extract/ingest, watch stays at started-ish snapshot (`completed_count` 0, optional stage label) until `finalize_job`. UI progress bars cannot move.

**Evidence:** Grep of process-runner shows `progress.patch` only at handler entry (plus mock test). No `get_checkpoint` loop during work (only `load_resume_params` at resume accept).

**Failure scenario:** Desk (**0020**) binds a progress bar to `watch_progress`; multi-minute PST extract appears frozen until complete/pause.

**Correction:** Either (a) runner-side poller of `get_checkpoint` + `completed_count` while job active, or (b) handlers/`ProgressSink` updates at stage checkpoint boundaries. Prefer (a) to avoid stage API churn (spec optional poller).

**Verification:** Test that forces multi-batch extract/ingest and asserts `completed_count` or stage snapshot changes before terminal.

**Deferrable:** No for DoD-4 as written (mid required). If product accepts start+terminal only for P0, re-scope DoD explicitly — do not silently ship.

---

### [P2] Preferred real-handler integration does not prove cancel→Paused→resume or extract success

**Confidence:** High

**Requirement:** DoD-8; plan Phase 6 “Integration: fixture extract; cancel + resume; assert one job row”; acceptance narrative cancel/resume

**Location:**
- `C:\dev\Dedupe\crates\process-runner\tests\integration.rs` — `extract_pst_fixture_via_runner`
- Same file — `ingest_zip_via_runner_one_job` (success only, no cancel/resume)
- Cancel/resume covered only by **mock** handlers (`cancel_mid_run_pauses`, `resume_continues_same_job_id`)

**Problem:**
1. Extract test accepts `Succeeded | Paused | Failed` and on Failed only `eprintln!` — a broken extract path still passes.
2. No runner test cancels a real `IngestHandler` / `ExtractPstHandler` and resumes the **same** `job_id` through checkpoints.
3. Silent `return` if no fixture (fixtures exist in-repo today, but CI without them is a no-op).

**Evidence:**
```rust
assert!(
    matches!(
        jobs[0].state,
        JobState::Succeeded | JobState::Paused | JobState::Failed
    ),
    ...
);
if jobs[0].state == JobState::Failed {
    eprintln!("extract failed (fixture?): {:?}", jobs[0].error_summary);
}
```

**Failure scenario:** Regression in cancel wiring for real stages (token not passed, wrong outcome mapping) is undetected; extract always-fail still green.

**Correction:**
- Assert extract non-Failed when fixture present (at least `messages_ok > 0` or Succeeded/Paused-with-checkpoint).
- Add runner integration: real ingest or extract → cancel → durable Paused + checkpoint → `resume` same id → Succeeded/Paused progress, still one job row.
- Prefer `#[ignore]` or hard fail over silent skip if fixtures are required in this repo.

**Verification:** `cargo test -p process-runner -- --nocapture` with fixtures.

**Deferrable:** No for cancel+resume proof on at least one real handler; extract success assert is required if fixture is part of preferred DoD.

---

### [P3] Drop/shutdown join timeout policy not documented for consumers

**Confidence:** High

**Requirement:** §3.2.1 / §3.3 — “document join timeout if any”

**Location:**
- `C:\dev\Dedupe\crates\process-runner\src\runner.rs` — “Join without a hard timeout”
- `C:\dev\Dedupe\crates\process-runner\README.md` — documents cancel + join, not the no-timeout policy

**Problem:** Non-cooperative handler hang blocks app exit indefinitely; consumers are not told.

**Correction:** Document in README: join waits until worker exits; no wall-clock timeout; stages must poll cancel.

**Verification:** Doc-only.

**Deferrable:** Yes (docs; behavior is intentional). Easy fix — do it before close.

---

## Completeness Sweep

| Pattern | process-runner | on_job paths |
|---|---|---|
| TODO / FIXME / stub / placeholder | None | None in inject APIs |
| `todo!` / `unimplemented!` | None | None |
| Handlers calling legacy `create_job` wrappers | None — only `*_on_job` / `resume_*` | Legacy wrappers still create job + delegate (back-compat OK) |
| Fake success / silent skip | Extract fixture test can soft-pass on Failed / missing fixture | — |

No incomplete Option C inject stubs found. Wiring is real code, not placeholders.

---

## Wiring Traces

### Start → terminal
`ProcessRunner::start` → accept lock + busy check → `Command::Start` → worker: handler lookup → `Matter::open` → `run_start`: **`create_job` + Running** → set `active` + cancel token → watch Started → reply `job_id` → `JobHandler::run` → `*_on_job` (no second `create_job`) → stage durable state → `finalize_job` (watch terminal) → clear `active` → drop `Matter`.

**Status:** Wired end-to-end. Error before reply broken (P1).

### Cancel → Paused
`cancel(job_id)` fast-path sets `ActiveJob.cancel` → handler/`as_fn` polled by stages → stage `set_job_state(Paused)` + summary `cancelled` → handler `JobOutcome::Paused` → finalize trusts durable Paused → watch `paused`.

**Status:** Correct for mock + stage design. Real-handler path not integration-tested via runner (P2).

### Resume → same job_id
`resume` → busy check → `run_resume`: `get_job` (same id) → load `source_id` from checkpoint → Running → handler `is_resume` → `resume_ingest` / `resume_extract` → finalize. No `create_job`.

**Status:** Wired. Mock-tested. Missing-job error path wrong (P1). Real resume via runner not tested (P2).

### Drop / shutdown → join
`shutdown`/`Drop`: cancel active → take cmd tx → `Shutdown` → **join** worker handle (no detach).

**Status:** Met; drop timing test present.

---

## Tests Assessment (DoD proof)

| Spec test intent | Coverage quality |
|---|---|
| Start + succeed one job | Strong (mock + real ingest) |
| Cancel → Paused + watch | Strong for **mock** only |
| Resume same job_id | Strong for **mock** only |
| Single job row / Option C | Strong (runner + stage crates) |
| Blocking / worker thread | Strong |
| Single-flight Busy | Strong |
| Watch latest / terminal | Adequate terminal; mid weak |
| Shutdown/Drop join | Adequate (time bound, not state assert) |
| Unknown kind | Strong |
| Real PST integration | **Weak** (Failed allowed; no cancel/resume) |

---

## Summary

Core architecture for **0019** is substantially in place and matches the design locks: single matter worker, Option C job-id injection with real `*_on_job` APIs, cooperative cancel → stage **Paused**, watch (not crossbeam), Drop cancel+join, single-flight Busy, and solid mock-level lifecycle tests plus a good real **ingest** one-job path.

**NEEDS_FIX** before engineering DoD can be considered met:

1. **P1** — reply-channel drop turns `JobNotFound` / early matter errors into `WorkerGone`.
2. **P2** — mid-run watch progress missing for real handlers (DoD-4).
3. **P2** — strengthen real-handler integration (extract success assert; cancel→resume through runner).
4. **P3** — document no join timeout.

Finalize items (DoD-10/11: workspace gates, `ledgerful verify`, `review.md`, conductor Completed) remain open by plan and are outside this code-defect list but block track completion.
