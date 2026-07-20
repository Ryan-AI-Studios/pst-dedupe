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

**Join timeout policy:** there is **no wall-clock join timeout**. `Drop`/`shutdown` wait until the matter worker exits. Stages **must** poll `CancelToken` so cancel leads to a cooperative **Paused** exit; a non-cooperative hang in a handler will block process exit. Do not detach the worker on app close — in-flight SQLite batches need a clean pause or finish.

## Mid-run progress

While a handler is blocked, a companion **progress-poller** thread opens a second Matter connection via **`Matter::open_for_read`** (SQLite WAL, **no** `workspace/temp` cleanup) and mirrors checkpoint `completed_count` for stages `expand` / `pst_extract` into the watch sink. Using full `Matter::open` in the poller is forbidden — it would race CAS PST materialization under `workspace/temp/`. Terminal snapshots are published when the handler returns.

## Durable single-flight

Besides the in-memory `active` slot, `start` rejects with `Busy` if any job row is already **Running** in SQLite (e.g. prior process crash). Resume that job or mark it Failed/Paused before starting another.

## Public API (sketch)

```rust
use std::sync::Arc;
use process_runner::{
    ProcessRunner, RunnerConfig, JobParams, IngestHandler, ExtractPstHandler,
    MatterDedupeHandler,
};

let mut runner = ProcessRunner::new(RunnerConfig::default());
runner.register(Arc::new(IngestHandler::new()));
runner.register(Arc::new(ExtractPstHandler::new()));
runner.register(Arc::new(MatterDedupeHandler::new()));

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
| `dedupe` | `MatterDedupeHandler` | `{ "use_message_id", "use_logical_hash", "family_policy", "reset", "batch_size" }` (all optional; defaults apply) | checkpoint `stage=dedupe` cursor |
| `thread` | `MatterThreadHandler` | `{ "use_headers", "use_subject_fallback", "use_conversation_index", "reset", "batch_size", "family_inherit" }` | checkpoint `stage=thread` cursor |
| `neardup` | `MatterNearDupHandler` | `{ "shingle_k", "cjk_char_n", "num_hashes", "num_bands", "rows_per_band", "threshold", "skip_exact_duplicates", "min_chars", "reset", "batch_size", "include_attachments", ... }` (all optional; defaults apply) | checkpoint `stage=neardup` cursor |
| `cull` | `MatterCullHandler` | `{ "preset_name": "unique_only", "preset_id", "rules", "reset", "batch_size" }` (all optional; defaults apply) | checkpoint `stage=cull` cursor |
| `promote` | `MatterPromoteHandler` | `{ "policy": "auto", "review_set_name", "expand_families", "reset", "batch_size", "require_dedupe" }` (all optional; defaults apply) | checkpoint `stage=promote` cursor |
| `produce` | `MatterProduceHandler` | `{ "scope": "review_corpus", "name", "bates_prefix": "PROD", "fail_if_withheld", "export_eml_if_missing_native", "include_csv_twin", "expand_family", "output_dir" }` (all optional; defaults apply). Alias kind `production_export` | checkpoint `stage=produce` cursor |
| `fts_index` | `MatterFtsIndexHandler` | `{ "reset": false, "batch_size": 100, "scope": "all_with_text", "writer_heap_bytes" }` (all optional; defaults apply) | checkpoint `stage=fts_index` cursor |
| `office_extract` | `MatterOfficeExtractHandler` | `{ "force": false, "batch_size": 50, "formats": ["docx","xlsx","pptx"] }` (all optional; defaults apply) | checkpoint `stage=office_extract` cursor |
| `pdf_extract` | `MatterPdfExtractHandler` | `{ "force": false, "batch_size": 50 }` (all optional; defaults apply) | checkpoint `stage=pdf_extract` cursor |
| `ocr` | `MatterOcrHandler` | `{ "force": false, "batch_size": 20, "lang": "eng", "max_pages": 500, "dpi": 200, "enabled": false, "engine": "tesseract", ... }` — fails closed when `enabled` is false | checkpoint `stage=ocr` cursor |
| `classify` | `MatterClassifyHandler` | `{ "force": false, "batch_size": 100, "use_magic": true, "in_review_only": false, "respect_extractor_refine": true }` | checkpoint `stage=classify` cursor |
| `profile_run` | `MatterProfileRunHandler` | `{ "profile_id" \| "profile_name", "stop_on_stage_failure": true }` | checkpoint `stage=profile_run` cursor (`stages: [{stage,job_id,status}]`) |
| `workflow_run` | `MatterWorkflowRunHandler` | `{ "workflow_id" \| "workflow_name", "run_params"?: object }` | checkpoint `stage=workflow_run` cursor (`nodes: [{node_id,job_id,status,…}]`) |

### `profile_run` (track 0043)

Sequential multi-stage apply of a named processing profile:

1. Resolve profile (`builtin:standard` or user id/name).
2. Plan = `CANONICAL_STAGE_ORDER ∩ enabled` (never JSON order).
3. For each stage: **`create_job_with_parent(kind, parent_job_id)`** → dispatch registered stage handler on the **same worker** → terminal child state.
4. Parent checkpoint lists `{stage, job_id, status}`; resume skips succeeded stages.
5. **Never** call `ProcessRunner::start` for children (would return Busy).

Built-in profiles use **cumulative** `reset: false`. Handlers skip already-processed items unless `reset`/`force`. Register with `MatterProfileRunHandler::with_default_handlers()` (includes feature-gated stages).

Profile stage idempotency: **promote** with `reset:false` re-expands membership (may add items); **cull** skips already-flagged rows. Corrupt `profile_run` checkpoints fail closed (no silent empty restart).

### `workflow_run` (track 0044)

Sequential multi-node apply of a named workflow (`list_workflows` = built-ins ∪ user):

1. Resolve workflow (`builtin:…` or user id/name); freeze body in checkpoint.
2. Bind node params with **AST-only** placeholder substitution from `run_params` (never raw JSON replace).
3. For each enabled node: create child job with **`parent_job_id` = workflow parent** (nested `profile_run` stages parent to the profile_run child).
4. Hard gates (`require_qc_pass`, `require_has_sources`) reject `soft_fail` at validation; runtime hard-fails.
5. **Never** call `ProcessRunner::start` for children (would return Busy).

Register with `MatterWorkflowRunHandler::with_default_handlers()`.

Register additional handlers with `JobHandler` for future tracks. Features `fts`, `office`, `pdf`, `calendar`, `ocr`, and `classify` are **default-on**.

## Option C (job-id injection)

```text
process-runner: create_job → set Running → handler(job_id)
ingest-purview:  ingest_path_on_job(..., job_id, ...)   // no create_job
extract-pst:     extract_pst_item_on_job(..., job_id, ...)
matter-dedupe:   run_dedupe(matter, job_id, ...)          // no create_job
matter-thread:   run_thread(matter, job_id, ...)          // no create_job
matter-neardup:  run_neardup(matter, job_id, ...)         // no create_job
matter-cull:     run_cull(matter, job_id, ...)            // no create_job
matter-promote:  run_promote(matter, job_id, ...)         // no create_job
matter-produce:  run_produce(matter, job_id, ...)         // no create_job
matter-search:   run_fts_index(matter, job_id, ...)       // no create_job
```

Legacy wrappers (`ingest_path`, `extract_pst_item`) still create a job then call `*_on_job` for CLI/tests that do not use the runner.

## Features

| Feature | Default | Dep |
|---|---|---|
| `ingest` | on | `ingest-purview` |
| `extract_pst` | on | `extract-pst` |
| `dedupe` | on | `matter-dedupe` |
| `thread` | on | `matter-thread` |
| `neardup` | on | `matter-neardup` |
| `cull` | on | `matter-cull` |
| `promote` | on | `matter-promote` |
| `produce` | on | `matter-produce` |
| `fts` | on | `matter-search` |
| `office` | on | `extract-office` |
| `pdf` | on | `extract-pdf` |

## Tests

```powershell
cargo test -p process-runner
```
