# 0120 — PdfRasterUiResiduals — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Ledger:** `ledgerful ledger start 0120-pdf-raster-ui-residuals --category BUGFIX --message "Image-frame draw coords + cancel in-flight drag + burn-set recount"`

---

## Phase 0 — Precondition / API gate → DoD-4

- [x] Re-verify `SCHEMA_VERSION == 41`. Re-read live `ui/src/pages/review_window.rs` (`event_to_raster` ~156, doc Effect ~490, raster Effect ~500, image-frame ~1290–1318, Escape ~844). Re-read `app.css` `.geom-overlay` `pointer-events`. Re-read `ui/src/pages/produce.rs` Burn step (~783–847) and `volume_succeeded` (do **not** edit). Re-read `crates/dedupe-chrome/src/raster.rs` `ProduceBurnSetResponse` + `burn_counts_for_ids` + `produce_burn_set_blocking`.
- [x] Re-read PR #119 Bugbot (wrong overlay coords / draw state / stale Burn counts). Confirm last-4 PRs still have no product findings.
- [x] Do **not** implement 0121–0126. Do **not** bump schema. Do **not** change 0114 compose. Do **not** set overlays to `pointer-events: none`.

## Phase 1 — Frame coordinates → DoD-1

- [x] `frame_css_point` + ui unit tests (client − rect origin). Clamp to rect bounds.
- [x] Rewrite `event_to_raster` (or replacement) to use `current_target` `get_bounding_client_rect` + `client_x`/`client_y`. Remove `offset_x`/`offset_y` from draw handlers.
- [x] mousedown / mousemove / mouseup on `.image-frame` all use that helper.
- [x] Keep the existing 2 **raster-px** min-drag guard (do not convert to CSS px).

## Phase 2 — Draw cancel → DoD-2

- [x] Clear `drawing` / `drag_origin` / `drag_now` on `doc_id` change, `raster_page_index` change (inside the raster Effect’s load path), **a separate Effect on `pane` leaving `"image"`**, and `mouseleave` of the frame (not `mouseout`).
- [x] Keep Escape clear. Mouseup is already a no-op when `!drawing`.

## Phase 3 — Burn recount → DoD-3

- [x] Clone the burned id list **before** the loop. `ProduceBurnSetResponse` (+ ui `ProduceBurnSet`) gains `need_burn` / `burned_fresh` / `unmapped_text` from `burn_counts_for_ids` on that clone. `None`/empty → skip recount (counts stay 0).
- [x] UI patches those three fields on `qc` when `Some`; keep `produce_page` refresh; do not drop findings / `ordered_ids`; do not re-run QC solely for counts.
- [x] Host test: after burning every required id in the set, response `need_burn == 0`.

## Phase 4 — Verify → DoD-4

- [x] `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml`
- [x] `cargo test -p dedupe-chrome`
- [x] trunk / chrome-ui still builds. Review Image tab / Produce Burn / Finalize latch still route.

## Phase 5 — Finalize → DoD-5

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (or ledgerful verify --scope full)
- [ ] CHANGELOG Unreleased sentence.
- [ ] Write `review.md` (commands, HITL overlay-release + page-change mid-drag + Burn counts).
- [ ] Update `../conductor.md` → **Completed**. Close `D-0120-pdf-raster-ui` in `docs/deferred.md`.
- [ ] Commit the BUGFIX ledger transaction.
- [ ] Owner HITL: release EXE, synthetic PDF token.

---

## Handoff notes

- Single-exe / no-daemon. Unique-pst is **not** this page.
- **0125** un-wizard stays Proposed. **0121** OPT QC stays Proposed. **0119** latch stays.
- 0120 `spec.md` / `plan.md` are already tracked. `git add -f` only if `git status` shows **untracked** `conductor/` files.
