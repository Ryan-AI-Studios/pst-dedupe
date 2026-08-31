# Track 0116 ProcessFold — review

- **Track:** `0116-ProcessFold`
- **Branch:** `track/0116-process-fold` (product) → `docs/0116-completed` (registry)
- **Registry:** **Completed**
- **Product PR:** **#123** squash-merged to `main` as `727c857`

Engineering complete. Registry **Completed** and deferred closeout land in this docs commit.

## Scope

Fold `process-runner` into `dedupe-chrome` Process workspace. Produce/QC start+cancel go through the same runner (closes **D-0113-long-job**). Schema stays **41**. `dedupe-desk` Process is unchanged. 0117–0121 stay Proposed placeholders (0121 spec/plan minted from PR #121 Bugbot, not implemented).

## DoD

| Item | Result |
|---|---|
| DoD-1 Process page live; four tabs; Desk builds | PASS — `ProcessPage` replaces stub; `cargo check -p dedupe-desk` / `pst-dedup-gui` / `pst-dedup-cli` |
| DoD-2 Golden path / Unaccounted-for | PASS — ingest zip + profile host test; Unaccounted-for uses `pst_extract` checkpoint `pst_item_id` vs inventory (fail closed); encrypted refused |
| DoD-3 One runner / Busy / cancel / orphan / reject export / isolate | PASS — `crash_recovery_resume_orphan_running`, `produce_start_while_ingest_running_is_busy`, `production_export` reject, `matter_id` isolation |
| DoD-4 Long jobs / findings / privilege-log / Busy banner / no `{}` | PASS — runner `start` returns `{ job_id }`; `produce_qc_findings`; host `ensure_privilege_log_after_produce` retries lock; Busy banner → Process |
| DoD-5 Honesty / no unwrap / schema 41 / no `connection()` | PASS — `list_item_errors_recent` cap 100 by `created_at`; Discovered = `top_level_items`; no process-runner on ui crate |
| DoD-6 Recorded | PASS — product PR **#123** / `727c857`. Registry **Completed**. Closes **D-0116-process-fold** and **D-0113-long-job**. Residuals **D-0116-workflow** / **D-0116-drop** / **D-0116-report**. CHANGELOG Unreleased sentence in product PR. |
| Owner HITL | Not CI — release EXE + synthetic matter remains owner. Codesign is **D-0062-codesign**. |

## Reviewer rounds

| Round | Verdict |
|---|---|
| Internal vs DoD | Product DoD-1…5 met; P2 privilege-log swallow and unaccounted fail-closed tightened |
| Codex r1 (`review.codex.r1.md`) | FAIL — P1 unaccounted-while-busy (false positive vs §3.2); P1 cross-matter durable lock (OOS); P2 cancel-after-start; P2 blank volume |
| Codex r2 (`review.codex.r2.md`) | FAIL — P2 extract-all skip on Busy; P2 produce lock vs Busy |
| Codex r3 (`review.codex.r3.md`) | FAIL — P1 DoD-6 (Phase 8, declined); P2 job-count unaccounted; P2 privilege-log lock race |
| Codex r4 (`review.codex.r4.md`, gpt-5.6-luna high) | **PASS WITH DEFERRED P3** — no open product P0–P2 |

Accepted P3 residuals (minted): **D-0116-workflow**, **D-0116-drop**, **D-0116-report**.

## Local gates (orchestrator-observed)

- `cargo fmt --all --check` pass
- `cargo clippy --workspace --all-targets -- -D warnings` pass
- `cargo test -p dedupe-chrome` **101 passed**
- `cargo test --workspace` pass
- `cargo check --target wasm32-unknown-unknown` in `crates/dedupe-chrome/ui` pass
- `ledgerful verify` alias: fmt+clippy pass; `cargo test --workspace` step **timed out at 300s** on first product commit. Fallback (already observed): `cargo test --workspace` exit 0 (~420s). Follow-up push ran verify including workspace tests in 244s.

## CI (required)

PR **#123** required checks green: fmt, clippy, test, audit, deny, chrome-ui, verify-parity. Follow-up commit `f82cf02` (folded into squash `727c857`) accepted `Ok(Unsupported, entries_ok=0, bytes_cas=0)` for Windows symlink-root ingest so CI (symlink privilege) matches “must not follow into CAS.”

## Drift vs spec

- `item_errors` recency is `created_at` (no `updated_at` at schema 41). Plan documents this.
- `profile_run_params` includes `stop_on_stage_failure: true` (Desk match).
- Produce `produce_page` skips privilege-log write when the runner still holds `.matter.lock` (`already open`); `process_progress` retries then continues.

## Residuals minted

- **D-0116-workflow** — no workflow picker
- **D-0116-drop** — dialog picker only (no drag-drop)
- **D-0116-report** — no reconciliation report download

Closed **D-0116-process-fold** and **D-0113-long-job**.

## Out of scope (honored)

No 0117–0121 product code. No schema bump. No `process-runner` on `ui/`. No BCC-default. No WASM jobs. Desk Process not gutted.
