# 0119 — ProduceChecklistResiduals — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Ledger:** `ledgerful ledger start 0119-produce-checklist-residuals --category BUGFIX --message "Empty privilege-log filter + Finalize disarm + matter QC reset + succeeded-only wait"`

---

## Phase 0 — Precondition / API gate → DoD-5

- [ ] Re-verify `SCHEMA_VERSION == 41`. Re-read live `ui/src/pages/produce.rs` (`wait_process_terminal` ~32, Effect ~93, Finalize `disabled` ~717, produce wait ~260, QC wait ~144, QC `start_result.set(None)` ~171/~178). Re-read `matter-core/src/privilege.rs` `if !ids.is_empty()` (~928, ~987). Re-read chrome `write_privilege_log_for_volume` (~655). Re-read **`crates/process-runner/src/handlers/produce.rs:93-94`** (Paused → paused / `"cancelled"`). Re-read `ensure_privilege_log_after_produce` (~600–616) silent-Ok on `already open`.
- [ ] Re-read PR #117 Bugbot (Finalize / empty filter / QC leak) and PR #123 cancelled-as-success.
- [ ] Do **not** implement 0120–0126. Do **not** bump schema. Do **not** weaken privilege-in-set.

## Phase 1 — Privilege-log empty-set → DoD-1

- [ ] `export_privilege_log` + `count_privilege_log_blank_descriptions`: `Some([])` → 0 rows / 0 blanks; `None` unchanged.
- [ ] matter-core `tests/privilege.rs` covers `None` vs `Some([])` vs `Some([id])`.
- [ ] Host still passes `Some(union)`. Empty-union blank count **0** is accepted (no corpus alarm at QC). Do **not** pass `None` from chrome.

## Phase 2 — Wait + Finalize + matter switch → DoD-2, DoD-3, DoD-4

**Order:** §3.1 helper + wait gates **before** §3.2 latch. Do not key `disabled` off `start_result.ok` (QC clears it).

- [ ] `process_job_succeeded` + ui unit tests. Produce and QC waits use it; after every await, ignore wait if `root_sig` drifted.
- [ ] Dedicated `volume_succeeded` latch: set true only on produce §3.1-succeeded refresh; QC must not clear it; not-succeeded leaves it; matter switch clears it.
- [ ] Finalize `disabled` when `volume_succeeded` or `start_busy` (plus existing gates). Apply `next_seq_hint` to `bates_start` **in that same success refresh only**.
- [ ] Route Effect clears `qc` / `overrides` / `start_result` / `volume_succeeded` / `entire_corpus` / `bates_start` / banners on root change. **Do not** reset `start_busy`/`qc_busy` in the Effect.
- [ ] Do not change runner Paused mapping. Do not fail `process_progress` by rewriting the `already open` silent-Ok.

## Phase 3 — Verify → DoD-5

- [ ] `cargo test -p matter-core --test privilege`
- [ ] `cargo test -p dedupe-chrome`
- [ ] `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml`
- [ ] trunk / chrome-ui still builds. Produce / Process / Review still route.

## Phase 4 — Finalize → DoD-6

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (or ledgerful verify --scope full)
- [ ] CHANGELOG Unreleased sentence.
- [ ] Write `review.md` (commands, HITL second Finalize + matter switch + cancel).
- [ ] Update `../conductor.md` → **Completed**. Close `D-0119-produce-checklist-residuals` in `docs/deferred.md`.
- [ ] Commit the BUGFIX ledger transaction.
- [ ] Owner HITL: release EXE, synthetic produceable set.

---

## Handoff notes

- Single-exe / no-daemon. Unique-pst is **not** this page.
- **0125** un-wizard stays Proposed. **0122** extract-all stays Proposed.
- 0119 `spec.md` / `plan.md` are already tracked. `git add -f` only if `git status` shows **untracked** `conductor/` files (the directory ignore still applies to new untracked tracks).
