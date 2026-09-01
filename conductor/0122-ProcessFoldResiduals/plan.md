# 0122 — ProcessFoldResiduals — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Ledger (execute):** After owner git-commits the Ready+foldin conductor/docs (not product), `ledgerful ledger start 0122-process-fold-residuals --category BUGFIX --message "Extract-all Busy must not wipe queue; job-row orphan/active from live progress"`

---

## Phase 0 — Precondition / API gate → DoD-3

- [x] Re-verify `SCHEMA_VERSION == 41`. Re-read live `ui/src/pages/process.rs` (`extract_all` ~382, drain ~264, `finished_paused` ~221, `snapshot_busy` ~106, `is_orphan_running` ~133, job `For` ~546, poll ~300). Re-read `error.rs` Busy Display. Re-read host `reject_if_busy` and `process_progress_blocking` idle synthesis (~348–363) (do **not** edit host Busy).
- [x] Re-read PR #123 Bugbot (extract-all Busy + live orphans). Confirm last PRs (#136–#133) still have no product findings. Cancelled-produce stays **0119**.
- [x] Owner git-commits tracked `conductor/0122-*` spec/plan/foldin-note + registry/deferred **before** the product BUGFIX tx. Do **not** `git add` repo-root `agy-review.md` or `fixtures/keep_set_summary.json`.
- [x] Do **not** implement 0123–0126. Do **not** bump schema. Do **not** change `process-runner` Busy.

## Phase 1 — Extract-all Busy → DoD-1

- [x] Helpers + ui unit tests: `extract_all_should_start`, `is_busy_invoke_err` (copy `produce.rs` `is_busy_err`: `busy:` **or** `matter is busy`), `should_clear_queue_on_start_err` (false for Busy), `should_set_busy_retry` / `should_fire_busy_retry` / `should_clear_busy_retry` (success, `finished_paused`, non-Busy clear).
- [x] `extract_all`: guard **before any signal writes**. Busy start Err does not clear queue/total; sets `busy_retry_pending`.
- [x] Drain next-start (`~288`): Busy does not `extract_queue.set(Vec::new())`; sets the flag. Fire `q.first()` without a second `remove(0)` only while flag set **and** `!snapshot_busy`. Clear flag on successful start and on `finished_paused` (Pause/Cancel must not auto-restart).
- [x] Optional: disable Extract all while `snapshot_busy` or `busy_retry_pending` — not solely because leftover queue after Pause.
- [x] If host has no Display test for `CommandError::busy`, add `assert!(format!("{e}").starts_with("busy:"))` next to existing `kind == "busy"` tests. Do not change `reject_if_busy`.

## Phase 2 — Job-row reactivity → DoD-2

- [x] Remove one-shot `let orphan` / `let active` / `let counts` from `For` children. Read `progress` inside `Show` / count views. Keep `is_orphan_running`.
- [x] Unit tests: matching running snap → not orphan; idle/other id → orphan.

## Phase 3 — Verify → DoD-3

- [x] `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml`
- [x] `cargo test -p dedupe-chrome` (Busy host tests still pass)
- [x] 0119 latch tests still pass. trunk / chrome-ui still builds.

## Phase 4 — Finalize → DoD-4

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace` (or ledgerful verify --scope full)
- [x] CHANGELOG Unreleased sentence.
- [ ] Write `review.md` (commands, HITL second Extract all, Pause must not auto-restart remaining queue, row Pause vs orphan Resume).
- [ ] Update `../conductor.md` → **Completed**. Close `D-0122-process-fold-residuals` in `docs/deferred.md`.
- [ ] Commit the BUGFIX ledger transaction.
- [ ] Owner HITL: release EXE, ≥2 inventory PSTs.

---

## Handoff notes

- Single-exe / no-daemon. Unique-pst is **not** this page.
- **0126** jobs table stays Proposed. **0119** cancelled-produce stays Completed.
- 0122 `spec.md` / `plan.md` / `foldin-note.md` are under `conductor/` (gitignore). `git add -f` if `git status` shows them **untracked**; they are already tracked on this tree.
- Do not `git add` stray repo-root `agy-review.md` or `fixtures/keep_set_summary.json`.
