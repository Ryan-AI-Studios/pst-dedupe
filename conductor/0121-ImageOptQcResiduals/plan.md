# 0121 — ImageOptQcResiduals — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Ledger (execute):** wait if an unrelated pending tx is open, then
> `ledgerful ledger start 0121-image-opt-qc-residuals --category BUGFIX --message "OPT skip until complete + scoped image QC + sniff magic before MIME"`

---

## Phase 0 — Precondition / API gate → DoD-6

- [ ] Re-verify `SCHEMA_VERSION == 41`. Re-read live `crates/matter-qc/src/rules.rs` (`evaluate_image_volume_rules`, OPT block, `is_qc_image_eligible`). Re-read `crates/matter-qc/src/params.rs` `QcParams`. Re-read `crates/pdf-raster/src/lib.rs` `sniff_kind` / `is_image_eligible_native`. Re-read `crates/matter-produce/src/run.rs` `check_image_fail_closed` + `write_image_opt`. Re-read chrome `intended_qc_params`.
- [ ] Re-read PR #121 Bugbot (OPT resume / all volumes / JPEG path / TIFF MIME). Confirm last-4 PRs still have no product findings.
- [ ] Do **not** implement 0122–0126. Do **not** bump schema. Do **not** change 0114 compose, 0115 G4 wrap, 0119 latch, or 0120 overlay/Burn recount.

## Phase 1 — OPT skip + set scope → DoD-1, DoD-2, DoD-3

- [ ] `QcParams.production_set_id: Option<String>` with serde default + skip-serialize-if-none. Default / `from_json("{}")` stays None. Pass from `run.rs` into `evaluate_image_volume_rules`.
- [ ] SELECT `status` on `production_sets`. OPT Error only for `complete` / `complete_with_errors`. Skip OPT on `running` / `partial` / `failed`.
- [ ] Set selection per spec §2.9 (explicit id; else in-progress intersecting; else exactly-one complete image set; else skip leftover completes). Skip disk TIFF checks when `output_root` is missing.
- [ ] Chrome: `intended_qc_params` / `produce_qc_run_blocking` pass set id when known (prefer non-complete thin set, else latest complete with existing root). Do not touch `volume_succeeded`.
- [ ] Keep `run_production_qc_image_pack_missing_opt_on_completed_volume`. Add resume (non-complete, pages, no OPT → pass) and leftover (two completes, one missing root → no Error from leftover).

## Phase 2 — sniff / eligibility → DoD-4, DoD-5

- [ ] Reorder `sniff_kind`: PDF, TIFF magic, JPEG magic, PNG magic, path, MIME.
- [ ] `is_image_eligible_native` = sniff is not Other. Remove path-only JPEG/PNG fallback.
- [ ] `matter-qc` depends on `pdf-raster`; `is_qc_image_eligible` calls `is_image_eligible_native` after native-only kind check. Delete path-first JPEG/PNG branch.
- [ ] Tests in `crates/pdf-raster/tests/g4.rs`: TIFF magic vs JPEG MIME (multi-IFD); path-only `.jpg` with Other bytes is not eligible / page count 0.
- [ ] `matter-produce` fail-closed: path-only jpg must not fail the volume.

## Phase 3 — Verify → DoD-6

- [ ] `cargo test -p matter-qc`
- [ ] `cargo test -p pdf-raster`
- [ ] `cargo test -p matter-produce`
- [ ] `cargo test -p dedupe-chrome`
- [ ] 0119 latch tests and 0120 overlay/Burn tests still pass.

## Phase 4 — Finalize → DoD-7

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (or ledgerful verify --scope full)
- [ ] CHANGELOG Unreleased sentence.
- [ ] Write `review.md` (commands, HITL cancel-resume + moved folder + garbage jpg + mis-tagged TIFF).
- [ ] Update `../conductor.md` → **Completed**. Close `D-0121-image-opt-qc` in `docs/deferred.md`.
- [ ] Commit the BUGFIX ledger transaction.
- [ ] Owner HITL: release EXE, synthetic image-profile matter.

---

## Handoff notes

- Single-exe / no-daemon. Unique-pst is **not** this page.
- **0122** Process extract-all stays Proposed. **0125** un-wizard stays Proposed.
- 0121 `spec.md` / `plan.md` are already tracked. `git add -f` only if `git status` shows **untracked** `conductor/` files.
- Series T / how-to-build Docs tx landed in PR **#133** / `6a69256`. Not this track.
