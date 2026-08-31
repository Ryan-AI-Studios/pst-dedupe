# 0116 — ProcessFold — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Ledger:** `ledgerful ledger start 0116-process-fold --category FEATURE --message "Fold process-runner into dedupe-chrome Process + produce/QC cancel"`

---

## Phase 0 — Precondition / API gate → DoD-3

- [x] Re-verify `SCHEMA_VERSION == 41`, `register_default_handlers` kinds, `builtin:standard` stages, `list_items_by_file_category("pst")`, `list_jobs`, item_errors helpers, chrome `ProcessStub`, produce `create_job`+`join_worker`, `capabilities/default.json`.
- [x] Re-verify `tauri` 2.x resolve (reject 3.x). `ui/` still excluded.
- [x] Confirm a synthetic ingest fixture exists (tiny zip or `fixtures/` PST) for host tests. Do not add client PSTs.
- [x] Findings reload is pinned: `produce_qc_findings(root, job_id: Option<String>)` — do not invent a second shape.
- [x] Do **not** depend on `dedupe-desk`. Do **not** bump schema. Do **not** implement 0117–0121.

## Phase 1 — Host runner + process commands → DoD-3, DoD-5

- [x] `dedupe-chrome` dep: `process-runner` path, default features.
- [x] `ProcessRunner` in Tauri state; `register_default_handlers`; `shutdown` on exit.
- [x] Commands: `process_page`, `process_start`, `process_progress`, `process_cancel`, `process_resume`. Encrypted first. Actor `"chrome"` on chrome-side audits only (runner rows stay `"system"`). Allow-list capabilities.
- [x] `process_start` allowlist: ingest / extract_pst / profile_run / qc / produce. **Reject** `production_export`.
- [x] `process_progress(root)`: idle unless `snapshot.matter_id` matches the matter opened from `root`.
- [x] Pure param JSON builders (ingest / extract_pst / profile_run) + unit tests. Copy shapes from Desk `params.rs`, do not import desk.
- [x] `Matter::list_item_errors_recent(&self, limit: u64)` — `&Matter` via `open_for_read`, cap **100** by `created_at` (table has no `updated_at`; schema stays 41). Chrome never `connection()`.
- [x] Assert `register_default_handlers` actually registered `produce` and `qc`, not just `default_handler_kinds()`.
- [x] Crash-recovery: durable Running orphan (snapshot idle) → `process_resume` same id **or** `process_cancel` unblocks later `process_start`.
- [x] Tests: ingest fixture, Busy, cancel, encrypted refuse, production_export reject, matter_id isolation, no `create_job` in new process module.

## Phase 2 — Process UI → DoD-1, DoD-2

- [x] Replace `ProcessStub` with three-pane Process page (sources + builtins, jobs + exceptions, report + reconciliation).
- [x] Dialog add folder/ZIP/PST → `process_start` ingest. Extract selected/all. Run `builtin:standard` (other three builtins selectable). Extract-all: UI `extract_queue`, **continue** on failure, copy “N of M extracted…”.
- [x] Poll progress. Cancel. Orphan Running row: Resume same id or Cancel. Status-bar sentence locked.
- [x] Unaccounted-for identity per spec §3.2. Discovered = **`top_level_items`** only (never `items_total`). Fail closed, never fake 0.
- [x] Open review-ready → 0111 route. No OST/MBOX/NSRL copy.
- [x] wasm chrome-ui: stub string gone; `ui/` has no process-runner dep.

## Phase 3 — Produce/QC off join_worker → DoD-4

- [x] `produce_qc_run` / `produce_start`: keep 0113 validation; then `process_start("qc"|"produce")`; return `{ job_id }` immediately. Remove chrome `create_job` for those kinds.
- [x] **Delete** `serde_json::to_value(...).unwrap_or_else(|_| json!({}))` — serialize fail → blocked, never `{}`.
- [x] Unit test: `intended_produce_params` JSON → `ProduceParams::from_json` deep-equal; same for `QcParams`.
- [x] Produce UI polls `process_progress`; refresh `produce_page` on terminal. Busy banner → Process tab / active job.
- [x] Host command `produce_qc_findings(root, job_id: Option<String>)` after QC terminal (wizard blockers/overrides still work).
- [x] Privilege-log.csv: **host** idempotent post-step on produce **success** (from `process_progress` / `produce_page` if volume lacks log). Not WASM, not UI-only, not inside the runner handler.
- [x] Test: produce/QC start does not wait for engine; Busy if Process job running.

## Phase 4 — Golden path + desk coexistence → DoD-2, DoD-1

- [x] Host integration: matter → ingest → extract → profile (or extract_only) → Processed > 0, Unaccounted-for 0 idle.
- [x] `cargo check -p dedupe-desk` / `pst-dedup-gui` still OK. Do not gut Desk Process.
- [x] Existing chrome review/produce/raster tests green.

## Phase 5 — Finalize → DoD-6

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (or ledgerful verify --scope full)
- [x] CHANGELOG Unreleased sentence.
- [ ] Write `review.md` (commands, fixture, HITL residual, deferred minted).
- [ ] Update `../conductor.md` → **Completed**. Close `D-0116-process-fold` + `D-0113-long-job` in `docs/deferred.md`.
- [ ] Commit the FEATURE ledger transaction.
- [ ] Owner HITL: release EXE, synthetic matter only.

---

## Handoff notes

- Single-exe / no-daemon. Jobs stay on the matter worker, never WASM.
- Unique-pst is **not** this page.
- If workflow / drop / report are skipped, mint `D-0116-*` in deferred (spec §9) — do not silently drop.
- **0117–0121** stay Proposed. Do not steal PR #121 image QC into this PR.
- `conductor/` new files need `git add -f` when the owner commits (directory is gitignored for untracked).
