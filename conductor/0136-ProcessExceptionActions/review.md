# 0136 — ProcessExceptionActions — Review

## Scope

Exception groups expose independent `sample_job_id` / `sample_item_id`. Retry only when the sample job is failed/paused. Resume errors surface (no `let _ =`). Exception title falls back to the raw code. No vault. Exclude stays absent. Failed/paused **job rows** also show Resume so `failed_unlogged` is actionable.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Honest empty state | PASS | “No item_errors recorded”; no fabricated counts. |
| DoD-2 Real groups; conditional Retry | PASS | Independent sample ids; `retry_allowed`; `spawn_resume` with error + `accepted_job`. |
| DoD-3 No vault or fake exclude | PASS | `EXCEPTIONS_NO_VAULT` copy. |
| DoD-4 Recorded | PASS | This file; PR **#150** / `a8287b4`. |

## Gates

Same Series V gate as 0133. Final Codex `review.codex-r2.md` **PASS**.

## Publish

- PR **#150** / `a8287b4`
- Closes **D-0136-exception-actions**
- Vault remains **D-0034-06** (never this track)
