# 0133 — Process unaccounted leaves — Plan

> Map phases to `spec.md` §7. Re-verify `unaccounted_for` + `extracted_pst_item_ids` at execute
> (`unaccounted_nonzero_when_pst_inventory_without_extract`, `golden_path_ingest_profile_unaccounted_zero`).
> Status: **Ready — not started**. Do not implement until `/implement-track`.
> Fold-in 2026-09-03: `opencode-review.md` + `agy-review.md`.

> **Ledger:** `ledgerful ledger start 0133-unaccounted-leaves --category FEATURE --message "name unextracted PST leaves on Process minus-stack"`

---

## Phase 0 — Pin formula → DoD-1

- [ ] Re-read `unaccounted_for`, `extracted_pst_item_ids`, `failed_jobs_without_item_errors` in `crates/dedupe-chrome/src/process.rs`.
- [ ] Confirm 0122 `extract_all_should_start` / `is_orphan_running(&job_for_orphan, …)` / `extract_queue.set(Vec::new())` count == 1 string-locks still in `ui/src/pages/process.rs`.
- [ ] Confirm WASM `ProcessPageResponse` in `ui/src/invoke.rs` still mirrors host.

## Phase 1 — Host DTO → DoD-1

- [ ] Add **exact** fields on host + WASM `ProcessPageResponse`:
  - `unextracted_psts: Vec<ProcessPstRow>` (ids not in `extracted_pst_item_ids`)
  - `failed_unlogged: u64` (same pass as `unaccounted_for`; WASM `#[serde(default)]`)
- [ ] Do **not** change `unaccounted_for` body. Do not rename the two fields.
- [ ] Host tests: existing unaccounted tests still pass; unextracted list length equals PST gap; `failed_unlogged` matches `failed_jobs_without_item_errors`.

## Phase 2 — UI + Extract remaining → DoD-2 / DoD-3

- [ ] Minus-stack: basenames from `unextracted_psts` via `strip_extended_path`. Footnote from `failed_unlogged > 0`, not `unaccounted_for - names`.
- [ ] **Extract remaining**: `extract_all_should_start` **before** queue writes; work list = `unextracted_psts` only. Start-failure path **must** call `apply_extract_start_err` / `should_clear_busy_retry` (same as extract-all). **Forbidden:** a second `extract_queue.set(Vec::new())`. Keep **Extract all**.
- [ ] **DoD-3 trigger:** progress poller reloads `process_page` when any source `status == "importing"` **and** snapshot is idle/terminal. Do not require `was_busy`. Immediate `start_kind` reload is not enough.
- [ ] UI tests: remaining queue ids == unextracted; `include_str` remaining still calls `extract_all_should_start` before writes; poller predicate includes the importing+idle reload.

## Phase 3 — Finalize → DoD-4

- [ ] `review.md`; registry Completed; ledger commit.
- [ ] HITL: two **synthetic** PSTs (not INC* in git). Record operator INC* as optional.

## Handoff

- Do not zero unaccounted to match the mockup.
- Extract remaining must not wipe `extract_queue` on Busy.
- Do not start extract automatically after ingest.
