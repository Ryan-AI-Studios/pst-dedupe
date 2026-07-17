# process-runner

In-process **matter job runner** for Dedupe Desk (track **0019**).

- **No daemons** — single process, single matter worker thread
- **Cancel** — cooperative `CancelToken` (`Arc<AtomicBool>`) → stage **Paused**
- **Progress** — `tokio::sync::watch` holds the **latest** snapshot (UI-safe; no queue back-pressure on the worker). Optional `broadcast` for full event streams (CLI/tests).
- **Option C job authority** — the runner is the **sole** creator of job rows for orchestrated runs; stage crates use `*_on_job` APIs that do **not** call `create_job`.

## Never run extract / ingest on the UI thread

```text
UI / CLI thread          matter-worker thread
─────────────────        ────────────────────
start / cancel /         open Matter
watch_progress    ──►    create_job → Running
                         handler (ingest/extract)
                         update watch
                         drop Matter
```

Desk (**0020**) must only call `start` / `resume` / `cancel` / `watch_progress` on the UI thread. Handlers block on the **matter worker** only.

## Concurrency (P0 locks)

| Policy | Choice |
|---|---|
| Worker | **Exactly one** matter-owning thread |
| Per matter | At most **one** Running job (second `start` → `Busy`) |
| Matter sharing | **Forbidden:** `Arc<Mutex<Matter>>` + rayon for P0 |
| Progress bus | **watch** latest; **not** crossbeam MPMC multi-sub |

## Drop / shutdown

`ProcessRunner::shutdown` and `Drop`:

1. Set cancel on the active job
2. Send `Shutdown` to the worker
3. **Join** the worker thread

Do not detach the worker on app close — in-flight SQLite batches need a clean pause or finish.

## Public API (sketch)

```rust
use std::sync::Arc;
use process_runner::{
    ProcessRunner, RunnerConfig, JobParams, IngestHandler, ExtractPstHandler,
};

let mut runner = ProcessRunner::new(RunnerConfig::default());
runner.register(Arc::new(IngestHandler::new()));
runner.register(Arc::new(ExtractPstHandler::new()));

let mut progress = runner.watch_progress();
let job_id = runner.start(
    matter_root,
    "ingest",
    JobParams::new(r#"{"path":"C:/exports/pkg"}"#),
)?;

// UI polls latest:
let snap = progress.borrow().clone();

runner.cancel(&job_id)?;
runner.resume(matter_root, &job_id)?;
runner.shutdown(); // or drop
```

### Job kinds (default features)

| Kind | Handler | Start params | Resume |
|---|---|---|---|
| `ingest` | `IngestHandler` | `{ "path": "…" }` | `source_id` from checkpoint / params |
| `extract_pst` | `ExtractPstHandler` | `{ "source_id", "pst_item_id" }` or `{ "source_id", "path" }` | `resume_extract` |

Register custom handlers with `JobHandler` for future tracks (0021 dedupe, …).

## Option C (job-id injection)

```text
process-runner: create_job → set Running → handler(job_id)
ingest-purview:  ingest_path_on_job(..., job_id, ...)   // no create_job
extract-pst:     extract_pst_item_on_job(..., job_id, ...)
```

Legacy wrappers (`ingest_path`, `extract_pst_item`) still create a job then call `*_on_job` for CLI/tests that do not use the runner.

## Features

| Feature | Default | Dep |
|---|---|---|
| `ingest` | on | `ingest-purview` |
| `extract_pst` | on | `extract-pst` |

## Tests

```powershell
cargo test -p process-runner
```
