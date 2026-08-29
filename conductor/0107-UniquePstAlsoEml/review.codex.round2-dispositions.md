# Codex audit round-2 dispositions — 0107 UniquePstAlsoEml

Source: fresh Codex FAIL @ `95da222`. Orchestrator-validated findings only.

| Finding | Disposition | What changed |
|---|---|---|
| [P1] `--also-eml` parent of `--out` can clear PST output | **Validated / Fixed** | Guard rejects when `out` is same-as/under `also_eml` before prepare. Volume sibling loop now includes primary (`1..=MAX`). Tests: `guard_rejects_also_eml_parent_of_out`, `also_eml_parent_of_out_is_usage_error_before_clear` (marker file survives `--overwrite`). |
| [P1] Summary-write failures can still omit `{dir}/summary.json` | **Validated / Fixed** | Helper returns `Err` on summary write failure (fail-closed) after best-effort rewrite with real pack counts. Wrapper only synthesizes hard-fail summary when no usable summary exists; hard-fail counts `.eml` files on disk. Quarantine rewrite returns `Result` (I/O propagated). Test: `summary_write_failure_returns_err`. |
| [P2] Production cancel/failure path tests | **Validated / Fixed** | Added `cancel_during_pst_write_skips_also_eml` via `run_unique_pst_with_options` (cancel on `stage=write`): exit 130, `also_eml_ran=false`, no pack manifest/EMLs. Residual: cancel-*during*-also-eml integration still not covered end-to-end (quarantine rewrite unit-tested). |

DoD-4 Completed / final `review.md` — still orchestrator-owned (not this pass).

Ledger FIX: `d8932c9e-d713-4def-927f-81cb67ba293b`
