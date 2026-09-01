# 0120 — PdfRasterUiResiduals — Review

## Scope

Image-tab draw honesty and Produce Burn-step counts after PR **#119** Bugbot: overlay mouseup in **image-frame** space, in-flight draw cancel, set-scoped Need-burn recount. Schema stays **41**. **0121–0126** product code not implemented. 0114 burn compose and 0119 `volume_succeeded` unchanged.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Frame coords | PASS | `frame_css_point` + `event_to_frame_css` / `event_to_raster` use `current_target` `getBoundingClientRect` + `client_x`/`client_y`. Draw mousedown/move/up do not use `offsetX`/`offsetY`. Clamp to rect. Keep 2 **raster-px** min-drag. Tests: `frame_css_point_subtracts_rect_origin`, `frame_css_point_clamps_to_rect`, `draw_handlers_use_frame_client_coords_not_offset`. Owner HITL remaining. |
| DoD-2 Draw cancel | PASS | `clear_in_flight_draw` on `doc_id` Effect, raster load path (after early-return), **own Effect** `pane != "image"`, `.image-frame` `mouseleave` (not `mouseout`). Escape still clears. Mouseup no-op when `!drawing`. Overlay `pointer-events: auto` unchanged. |
| DoD-3 Burn counts | PASS | Clone ids before loop; `ProduceBurnSetResponse` + UI `ProduceBurnSet` gain `need_burn` / `burned_fresh` / `unmapped_text` from `burn_counts_for_ids`. `None`/empty skip (counts 0). UI `patch_qc_burn_counts` then `produce_page`. Findings / `ordered_ids` stay. Host: `produce_burn_set_response_need_burn_matches_set_after_burn`; `empty_burn_set_skips_recount_counts_stay_zero`. |
| DoD-4 Hygiene | PASS | No new production `unwrap`/`expect`. Schema 41. 0118 `fetch_is_current` and 0119 latch tests still pass. ui tests 22; `dedupe-chrome` 112; wasm check; trunk 0.21.14 `--release` success. |
| DoD-5 Recorded | PASS | This file; registry **Completed**; `D-0120-pdf-raster-ui` closed. Ledger BUGFIX `fdfd1762` committed on the product squash. |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` | 22 passed |
| `cargo test -p dedupe-chrome` | 112 passed |
| `cargo check --target wasm32-unknown-unknown` (ui) | pass |
| `trunk build --release --config Trunk.toml` (ui) | success |
| `cargo test --workspace` | pass |
| `ledgerful verify` | pass (pre-push: fmt + clippy + workspace 270.7s) |
| CI (PR **#131**) | fmt, clippy, test, audit, deny, chrome-ui, verify-parity **green**. Bugbot NEUTRAL (skipping). |
| Codex r2 | **PASS**, no findings |

## Reviewer rounds

1. Internal: DoD-1…3 wired; schema / overlay `pointer-events` / 0118–0119 fences held. **PASS**.
2. Codex r1: FAIL — P2 remaining workspace clippy/test and exact trunk not yet run. No code defects.
3. Codex r2: **PASS**. Fresh pass after those gates; no open >low.

## HITL (owner)

Release chrome EXE, synthetic PDF with a visible token: draw a box that **releases over an existing overlay** → Burn must excise the dragged token, not a neighbor. Change page mid-drag → no geom from the old origin. Burn-step Need burn must drop after a successful set burn. Codesign is **D-0062-codesign**. INC* unique-pst is not a gate.

## Publish

- Branch: `track/0120-pdf-raster-ui-residuals`
- PR: **#131**
- Merge SHA: `e87f4c192c4d2e68d5ce3a9b21dad600885ddced`
