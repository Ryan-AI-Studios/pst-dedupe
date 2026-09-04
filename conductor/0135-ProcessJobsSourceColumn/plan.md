# 0135 — Process jobs Source column — Plan

> Status: **Ready — not started**. Fold-in 2026-09-03: `opencode-review.md` + `agy-review.md`.

> **Ledger:** `ledgerful ledger start 0135-jobs-source-column --category FEATURE --message "Process job rows show PST basename"`

---

## Phase 0 — Pin recovery path → DoD-1

- [ ] Re-read `ProcessJobRow`, `Job` select cols, `ExpandCursor`, `ExtractCursor`, 0122 orphan lock, `jobs_table_emdash_per_row_columns` (`{j.kind.clone()}`).
- [ ] Confirm no `params_json` on `jobs` (schema 41). Confirm `ingest_path_on_job` still `insert_source` before expand.

## Phase 1 — Host + UI → DoD-1 / DoD-2

- [ ] Checkpoint lookups **only** when `kind` is `ingest` or `extract_pst`.
- [ ] Ingest label:
  1. `get_checkpoint(id, "expand")` → `source_id` → `Source.path` basename (success / mid-expand).
  2. If no checkpoint: `list_sources()` unambiguous leftover (exactly one unlabeled ingest job and one source not claimed by another ingest checkpoint) → that path. Else `None` (do not pair two failures by clock).
- [ ] Extract label: `ExtractCursor.pst_path` leaf name; `open_fs_path` may be `None` (strip `\\?\` only on that field); else inventory path via `pst_item_id`.
- [ ] WASM `source_label: Option<String>` with `#[serde(default)]`.
- [ ] UI: label when Some **and** `{j.kind.clone()}` still in the Source cell (name + kind subline). Keep Pause/Resume/Cancel and `job_for_orphan` string-lock.
- [ ] Tests: ingest + expand checkpoint → basename; ingest **no** checkpoint + single leftover source → basename; two unlabeled ingests → kind fallback; extract before first checkpoint → kind fallback allowed; `{j.kind.clone()}` still in the row; Dupes/NIST `—`.

## Phase 2 — Finalize → DoD-3

- [ ] `review.md` (note: `profile_run` children keep kind in Source; no parent inherit); ledger commit.

## Handoff

- Never paint `page.dupes` on a job row.
- Never schema-bump for params.
