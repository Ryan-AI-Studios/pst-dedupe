# 0125 — ProduceCanvas — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Ledger (execute):** After owner git-commits this Ready spec/plan if still dirty, `ledgerful ledger start 0125-produce-canvas --category FEATURE --message "Un-wizard produce canvas, protocol, Stage pane, Finalize on blockers"`

---

## Phase 0 — Precondition / API gate → DoD-4

- [x] Re-verify `SCHEMA_VERSION == 41`. Re-read `produce.rs` (ui + host): `step` Shows, `.produce-foot` Finalize, `volume_succeeded` helpers, `ProducePageResponse`, `fail_if_withheld`. Re-read `app.css` `.produce-layout`. Re-read mock `.panes-3-produce` (research only).
- [x] Confirm 0119 `include_str` latch tests still name the helpers. Do **not** edit `queue_window.rs`. Do **not** implement 0126.
- [x] Confirm last PRs (#142–#139) still have no product findings.

## Phase 1 — Un-wizard + protocol + sets → DoD-1, DoD-2

- [x] Three-pane CSS (`236px minmax(0,1fr) 320px`). All five step panels visible (no exclusive `Show when=step==N` for bodies). Optional `#step-1-set` … `#step-5-preflight` if the `ol` jumps.
- [x] Drop step-5 tab auto-`run_qc`. Do **not** auto-run on mount. Pre-flight + Stage: **QC not yet run — click Re-run QC** until `qc` is Some. Keep the Re-run button.
- [x] Protocol block from `get_privilege_protocol` (additive `produce_page` fields). Empty notes → **none on file**. No EDRM invention. UI `invoke.rs` new fields `#[serde(default)]`.
- [x] Additive `pad_width` on `ProductionProfileThin` from `p.body.bates.pad_width` (ui default too). Do **not** call `resolve_produce_config` only to display pad.
- [x] **New**: `if start_busy || qc_busy { return }`; clear QC/overrides/`start_result`/`volume_succeeded`/`bates_start` (`""`); restore DAT-only. Set rows = live `ProductionSetThin` only.
- [x] `include_str` / ui test: five `.produce-step` (or equivalent) visible; latch strings still present; no exclusive `Show when=step==N` bodies.

## Phase 2 — Stage + Finalize + shell slots → DoD-3

- [x] 320px Stage: live counts only; Pages/Slipsheets `"—"` if missing; export paths from selected profile; categorical still disabled. QC-not-yet-run copy when `qc` is None.
- [x] Move Finalize from `.produce-foot` into Stage; **same** disabled predicate + latch + click no-op. Omit/disable Stage & snapshot.
- [x] Number hint / projected last Bates use the **live prefix** signal, not `"PROD"`. Pad from Thin. Projected last Bates only from known `ordered_ids` + start.
- [x] If filling 0123 Produce slots: `wrap_produce` **creates** `ProduceChromeCtx` (labels only); `ProducePage` writes via `use_context`. Do **not** hoist Finalize/`qc`/`volume_succeeded`. Do not steal `QueueChromeCtx`. Do not change `PRODUCE_FLAG`. Keep `shell_source_locks` literals.

## Phase 3 — Verify → DoD-4

- [x] `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` (must include `shell_source_locks`)
- [x] `cargo test -p dedupe-chrome` (0119 latch + privilege-in-set + empty-union + 0124 queue locks)
- [ ] trunk / chrome-ui still builds.

## Phase 4 — Finalize → DoD-5

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (or ledgerful verify --scope full)
- [ ] CHANGELOG Unreleased sentence.
- [ ] Write `review.md` (commands, HITL five-visible + latch + none-on-file).
- [ ] Update `../conductor.md` → **Completed**. Close `D-0125-produce-canvas`.
- [ ] Commit the FEATURE ledger transaction.
- [ ] Owner HITL: release EXE.

---

## Handoff notes

- Single-exe / no-daemon. Unique-pst is **not** this page.
- **0126** stays Proposed. **0119** latch stays Completed.
- `git add -f` only if `conductor/0125-*` shows **untracked**.
- Do not `git add` stray repo-root `agy-review.md` or `fixtures/keep_set_summary.json`.
