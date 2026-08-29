# 0109 AlsoEmlClassifyHonesty — Review

**Status:** In progress (implement done; publish/PR left to orchestrator)  
**Branch:** `track/0109-also-eml-classify`  
**Closes:** `D-0109-also-eml-classify`  
**Does not close:** `D-0067-embedded-depth`, `D-0108-keepset-crc-retaint`

## Objective delivered

Restore 0078 honesty for unique-pst + `--also-eml`: combined `fidelity` is worse-of classified fidelities (not from exit); `ok == (fidelity == complete) && !cancelled`; also-eml cancel + summary rewrite keeps exit **130**; cancel `Err`→`Ok` recovers attach/embedded counts from summary JSON.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| **DoD-1** Combined classify | **PASS** | Deleted `match combined_exit` fidelity rewrite; `worse_export_fidelity` + `finalize_unique_pst_classify`; `ok` from fidelity; pack exposes `fidelity`; `artifact_state` pre-merge; risk 65 stays Complete |
| **DoD-2** Cancel rewrite | **PASS** | `classify_after_summary_write_failure(..., process_cancelled)`; cancel → 130 / CANCELLED / retryable; report-fail → Generic 1 |
| **DoD-3** Cancel counts | **PASS** | Cancel Ok uses `also_eml_recovered_counts`; recovery test asserts 7/2/3 |
| **DoD-4** Tests | **PASS** | See test names below; existing 0078 / 0107 also-eml green |
| **DoD-5** Docs | **PASS** | `docs/unique-pst-export.md` combined-job + allow-partial `ok=false`; CHANGELOG Unreleased; D-0109 closed |
| **DoD-6** Recorded | **Pending publish** | This file drafted; registry stays **In progress** until orchestrator merge; ledger tx `40364b99-…` not committed here |

## Bugbot IDs closed

| Finding | Fix |
|---|---|
| (M) partial marked complete / `ok` from exit 0; risk 65→failed | fidelity worse-of + call-site `ok` from fidelity |
| (M) summary rewrite drops also-eml cancel 130 | `process_cancelled` into classify + retryable |
| (L) cancel recovery zeros attach/embedded | `also_eml_recovered_counts` on cancel Ok |

## Tests (green)

```
cargo test -p pst-dedup-cli --lib export_outcome
  worse_export_fidelity_order
  finalize_allow_partial_also_eml_stays_partial
  finalize_allow_partial_without_also_eml
  finalize_risk_gate_complete_stays_complete
  finalize_eml_partial_marks_combined_partial
  finalize_also_eml_cancel_failed_fidelity
  classify_after_summary_write_failure_preserves_also_eml_cancel
  classify_after_summary_write_failure_report_fail_not_cancel
cargo test -p pst-dedup-cli --lib unique_eml_cmd
cargo test -p pst-dedup-cli --lib unique_pst_cmd
cargo test -p pst-dedup-cli --test unique_pst_also_eml
  cancel_ok_recovers_attach_and_embedded_from_summary
  helper_cancel_with_blocked_summary_returns_cancelled_ok
  cancel_during_pst_write_skips_also_eml
cargo test -p pst-dedup-cli --test export_exit_0078
cargo fmt --all --check
cargo clippy -p pst-dedup-cli --all-targets -- -D warnings
```

## Residual / deferred leftovers

- None from 0109 Bugbot trio.
- Frontend Series O remains **0110+**.
- No BCC-default track. No schema bumps. No `also_eml_fidelity` key.

## Publish

| Field | Value |
|---|---|
| PR | _(orchestrator)_ |
| Merge SHA | _(orchestrator)_ |
