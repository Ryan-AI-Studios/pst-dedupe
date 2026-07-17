# 0019-ProcessJobRunner — Review

- **Track:** 0019-ProcessJobRunner
- **Status:** Completed — Codex **PASS**
- **Date:** 2026-07-17
- **Crate:** `crates/process-runner` (+ Option C inject in `ingest-purview` / `extract-pst`; `Matter::open_for_read` / `list_jobs`)

## Summary

In-process matter job runner for Dedupe Desk:

| Area | Result |
|---|---|
| Worker | **Single** `matter-worker` thread owns `Matter` per job (no `Arc<Mutex<Matter>>` + rayon) |
| Jobs | **Option C:** runner sole `create_job`; stages use `*_on_job` |
| Cancel | `CancelToken` (`Arc<AtomicBool>`) → stage **Paused** |
| Progress | `tokio::sync::watch` latest snapshot; optional broadcast; mid-run via `open_for_read` poller |
| Single-flight | In-memory `active` + durable **Running** row → `Busy` |
| Shutdown | `Drop` / `shutdown` cancel + **join** (no wall-clock timeout; documented) |
| Handlers | `IngestHandler`, `ExtractPstHandler` (default features) |

## Public API

### process-runner
- `ProcessRunner::new/register/start/resume/cancel/watch_progress/subscribe_events/active_job/shutdown/wait_until_idle`
- `CancelToken`, `JobHandler`, `JobContext`, `JobOutcome`, `JobParams`, `JobProgressSnapshot`
- `IngestHandler`, `ExtractPstHandler`

### Stage inject (Option C)
- `ingest_path_on_job` — no `create_job`
- `extract_pst_item_on_job` / `extract_pst_path_on_job` — no `create_job`
- Legacy wrappers create job then call `*_on_job`

### matter-core
- `Matter::open_for_read` — concurrent readers without `workspace/temp` cleanup
- `Matter::list_jobs`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all --check` | **PASS** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** (pre-commit + targeted) |
| `cargo test -p process-runner` | **PASS** (1 unit + 14 integration) |
| `cargo test -p matter-core` | **PASS** (18) |
| `cargo test -p extract-pst` / `ingest-purview` | **PASS** |
| `cargo test --workspace` | **PASS** (pre-commit hygiene) |
| `ledgerful verify` | run at finalize |

## Review loop

| Round | Verdict | Notes |
|---|---|---|
| Internal R1 | NEEDS_FIX | reply→WorkerGone; mid progress; weak real cancel/resume |
| Internal R2 | **CLEAN** | reply + poller + tests |
| Codex R1 | **FAIL** | poller `Matter::open` temp race; durable Busy; active_job; cancel proof |
| Codex R2 | **PASS** | open_for_read; durable Running Busy; active_job filter; force Paused cancel |

## Design locks (confirmed)

- watch not crossbeam MPMC
- Option C job inject (no double job)
- single matter worker
- Drop join
- cancel → Paused

## Deferred (`docs/deferred.md`)

| ID | Item |
|---|---|
| D-0019-01 | Multi-job parallel stages inside one matter |
| D-0019-02 | Full `pst-dedup-cli job` subcommands (example `run_job` only) |
| D-0019-03 | Extract cancel→resume via runner (ingest path proven; extract fixture success proven) |
| D-0018-04 | Closed by this track (runner + progress) — still **0020** for Desk UI |

## Unblocked

- **0020** DeskShellUx — `watch_progress` + start/cancel only on UI thread
- **0021** MatterDedupeJob — register `JobHandler` without runner rewrite
