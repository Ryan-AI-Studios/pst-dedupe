# 0019 Internal Review R2

## Verdict: CLEAN

Re-review after fix commit `74b2379` (reply channel, mid-run poller, tests, docs). Read-only code audit of `C:\dev\Dedupe` on branch `feat/0019-process-job-runner`. Gates (`cargo test`, clippy, `ledgerful verify`) were **not** executed in this pass — DoD-10/11 remain finalize.

---

## Prior findings disposition

| R1 finding | Severity | Disposition | Evidence |
|---|---|---|---|
| Early `start`/`resume` errors drop reply → `WorkerGone` | P1 | **Closed** | `run_start` / `run_resume` always reply via `reply_err_string` / `reply_err_unit` (or `reply.send(Ok(...))`) before return; worker no longer relies on drop for typed errors. Test `resume_missing_job_is_job_not_found_not_worker_gone` asserts `JobNotFound`, not `WorkerGone`. Matter open failures also `reply.send(Err(e))`. |
| Mid progress missing | P2 | **Closed** | Companion `progress-poller` thread (`start_checkpoint_poller`) opens a second Matter connection (WAL) and mirrors `expand` / `pst_extract` checkpoint `completed_count` into the watch sink while the handler blocks. Poller stopped + joined before `finalize_job`. Test `mid_run_watch_reflects_checkpoint_progress` asserts mid-run `running` + `completed_count >= 1` + stage `expand`. README § Mid-run progress documents the design. |
| Weak extract / cancel-resume tests | P2 | **Closed** | `extract_pst_fixture_via_runner`: fixtures required (`.expect`), Failed no longer accepted — only `Succeeded \| Paused`; Paused requires `pst_extract` checkpoint. New `ingest_cancel_then_resume_same_job`: real `IngestHandler`, cancel → Paused + expand checkpoint → `resume` same `job_id` → Succeeded, still one job row. Mock cancel/resume remain. Soft early-return if ingest finishes before cancel is an escape hatch only (see notes). |
| Join timeout not documented | P3 | **Closed** | README **Join timeout policy**: no wall-clock join timeout; Drop/shutdown wait until worker exits; non-cooperative hang blocks process exit. Code comment in `shutdown` points at README. |

---

## New findings (if any)

No new **P0–P2** findings.

### Residual notes (non-blocking, not findings)

1. **`get_job` error mapping** — `run_resume` maps **any** `get_job` `Err` to `JobNotFound` (including rare SQLite failures). Missing-id path is correct and tested; imprecise mapping of other errors is P3-quality polish only.
2. **`ingest_cancel_then_resume_same_job` soft path** — if the job reaches `Succeeded` before cancel lands, the test returns after one-job assert and **skips** resume. With 200 zip entries + cancel on `is_busy`, the Paused→resume path is the intended common case; the escape hatch is honest, not a silent green on Failed.
3. **Extract cancel/resume** — not integration-tested via runner; real cancel→resume is proven on **ingest**. Extract path is proven for start/terminal/non-Failed + Option C. Adequate for DoD-8 preferred integration.
4. **DoD-10/11** — workspace gates, `ledgerful verify`, canonical `review.md`, conductor Completed: finalize only (unchanged from R1).

---

## DoD Matrix (code)

| DoD | Status | Evidence | Tests | Gap |
|---|---|---|---|---|
| **DoD-1 — Crate** | Met | `crates/process-runner` workspace member; features `ingest` / `extract_pst` default-on | Integration suite under `tests/integration.rs` | Gates not run here |
| **DoD-2 — Matter worker boundary** | Met | Single `matter-worker` thread; `Matter::open` only inside worker; handlers run there; no rayon/`Arc<Mutex<Matter>>` for jobs | `handler_runs_on_worker_not_caller` | — |
| **DoD-3 — Lifecycle** | Met | `create_job` → Running → handler → `finalize_job`; cancel → Paused; resume same `job_id`; early accept errors typed | Mock succeed/cancel/resume; real ingest succeed + cancel→resume; extract fixture | — |
| **DoD-4 — Progress** | Met | `tokio::sync::watch` primary; optional broadcast; no crossbeam; start + mid (poller) + terminal | Terminal watch; mid-run checkpoint poller test; ingest terminal | — |
| **DoD-5 — Option C** | Met | Handlers only `*_on_job` / `resume_*`; stage wrappers create job for back-compat | Stage on_job tests + runner one-job tests | — |
| **DoD-6 — Cancel + Drop join** | Met | `CancelToken` = `Arc<AtomicBool>`; stages polled; `shutdown`/`Drop` cancel + join; join policy documented | `cancel_mid_run_pauses`; `drop_joins_without_hang`; real ingest cancel | — |
| **DoD-7 — Single-flight / errors** | Met | `Busy` on second start; `UnknownKind`; `JobNotFound` on missing resume id | All three tested | — |
| **DoD-8 — Tests** | Met | Cancel + resume + single job_id + shutdown join; real ingest cancel/resume; extract fixture non-Failed | See above | Prefer extract cancel/resume later (not blocking) |
| **DoD-9 — Docs** | Met | process-runner README (worker, watch, Option C, Drop join + no timeout, mid-run poller); ARCHITECTURE + root/stage READMEs | — | — |
| **DoD-10 — Workspace gate** | Not verifiable | Not executed this pass | — | Finalize |
| **DoD-11 — Recorded** | Unmet | No canonical `review.md`; conductor Completed / ledger TX open | — | Finalize |

### Design locks (§3)

| Lock | Status |
|---|---|
| Option C sole job creator | Met |
| Single matter worker | Met |
| watch not crossbeam | Met |
| Drop join | Met |
| Single-flight | Met |
| cancel → Paused | Met |
| Mid progress (poller or handler) | Met (poller) |

### Wiring (post-fix)

| Path | Status |
|---|---|
| Start → terminal | Wired; early accept always replies typed Ok/Err |
| Cancel → Paused | Wired; mock + real ingest integration |
| Resume same job_id | Wired; mock + real ingest; missing id → `JobNotFound` |
| Mid-run watch | Poller mirrors checkpoints into watch |
| Drop/shutdown → join | Met; no-timeout policy in README |

---

## Summary

All four R1 findings are **closed** with concrete code, tests, and docs:

1. **P1 reply channel** — accept-phase errors always `reply.send(Err(...))`; missing resume id returns `JobNotFound`.
2. **P2 mid progress** — WAL second-connection checkpoint poller + mid-run watch test.
3. **P2 integration strength** — extract fixture must not Fail; real ingest cancel → Paused → resume same job_id.
4. **P3 join docs** — README documents no wall-clock join timeout.

Engineering DoD-1..9 are **met** from static review. No new P0–P2 defects found. Residual notes are polish/escape-hatch quality only.

**Track completion still requires finalize (DoD-10/11):** run workspace gates + `ledgerful verify`, write canonical `review.md`, mark conductor Completed + ledger TX. This R2 pass does not claim those executed.
