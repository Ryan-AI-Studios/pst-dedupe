# 0135 — ProcessJobsSourceColumn — Review

## Scope

Jobs Source column uses checkpoint `source_id` → `Source.path` basename, else unambiguous leftover ingest, else extract `pst_path` / inventory. Does **not** use `open_fs_path` as the Source name. `{j.kind.clone()}` remains. Dupes/NIST/Families/Except. stay `—`. Schema 41; no `params_json`.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Job source basename + kind fallback | PASS | Checkpoint lookup ingest/extract_pst only; two unlabeled ingests stay kind fallback. |
| DoD-2 Orphan and em-dash locks | PASS | Exact `is_orphan_running(&job_for_orphan, &progress.get())`; one `{j.kind.clone()}`; one queue wipe. |
| DoD-3 Recorded | PASS | This file; PR **#150** / `a8287b4`. |

## Gates

Same Series V gate as 0133. Final Codex `review.codex-r2.md` **PASS**.

## Publish

- PR **#150** / `a8287b4`
- Closes **D-0135-jobs-source-column**
