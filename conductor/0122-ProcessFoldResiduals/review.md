# 0122 — ProcessFoldResiduals — Review

## Scope

Process extract-all / live job-row honesty after PR **#123** Bugbot: a Busy second Extract all must not empty the remaining PST queue; a `running` job must show row Pause once `process_progress` has the matching `job_id`. Schema stays **41**. `process-runner` Busy unchanged. **0119** cancelled-produce, **0121** OPT/QC, and **0126** jobs-table layout not implemented. **0123–0126** stay Proposed.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Extract-all Busy | PASS | Guard before any `extract_queue` writes. Busy `process_start` (extract-all and drain next-start) keeps queue/totals via `extract_start_err_effect`. `busy_retry_pending` fires `q.first()` without a second `remove(0)` only after an explicit Busy failure. Pause/Cancel (including before the first poll) clears via `snapshot_clears_busy_retry`. Overlapping 400 ms polls `take_busy_retry_fire` before any await. UI tests in `extract_all_busy_tests`. Owner HITL remaining (release EXE, ≥2 PSTs). |
| DoD-2 Live row Pause | PASS | `For` children no longer freeze `orphan` / `active` / `counts`. Pause, counts, and orphan `Show` read `progress` reactively. `is_orphan_running` still covers idle/other-id true orphans. |
| DoD-3 Hygiene | PASS | No new production `unwrap`/`expect`. Schema 41. Host `CommandError::busy` Display `starts_with("busy:")`. `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` and `cargo test -p dedupe-chrome` (Busy + 0119 latch) passed. |
| DoD-4 Recorded | PASS | This file; registry **Completed**; `D-0122-process-fold-residuals` closed. Ledger BUGFIX `9d722d33` committed on the product squash. |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` | pass (27, including 6 extract-all Busy tests) |
| `cargo test -p dedupe-chrome` | pass (114, including `busy_display_starts_with_busy_colon`, `second_start_while_running_is_busy`, produce latch, `schema_stays_41`) |
| `cargo test --workspace` | pass |
| `ledgerful verify` | pass |
| CI (PR **#137**) | fmt, clippy, test, audit, deny, chrome-ui, verify-parity **green**. Bugbot NEUTRAL (does not block). |
| Codex r3 | **PASS**, no product findings |

## Reviewer rounds

1. Internal: DoD-1…3 wired; schema / runner / 0119–0121 fences held. Residual lows (Pause-order and drain keep-queue tests) fixed in-tree. **PASS** (no >low).
2. Codex r1: FAIL — Pause/Cancel before first poll could auto-retry; Busy keep-queue tests did not mutate a queue.
3. Codex r2: FAIL — overlapping 400 ms polls could dispatch the same retry twice.
4. Codex r3: **PASS**. Fresh pass after `snapshot_clears_busy_retry`, `extract_start_err_effect`, and `take_busy_retry_fire`; no open >low.

## HITL (owner)

Release chrome EXE, synthetic matter with **≥2** inventory PSTs: (1) Extract all, click Extract all again while the first extract is running — remaining PSTs must still dispatch after the in-flight job; (2) Pause the in-flight extract mid-batch — remaining queue must **not** auto-restart; (3) a live `running` job row must show **Pause** (not orphan Resume) once `process_progress` has the matching `job_id`. INC* unique-pst is not a gate. Codesign is **D-0062-codesign**.

## Publish

- Branch: `track/0122-process-fold-residuals`
- PR: **#137**
- Merge SHA: `f1810fe434f9b5e1bfee6bbb3e2ddec4ea3a3712`
