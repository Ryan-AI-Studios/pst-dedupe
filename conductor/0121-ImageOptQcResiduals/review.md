# 0121 — ImageOptQcResiduals — Review

## Scope

Image OPT / QC honesty after PR **#121** Bugbot: skip `opt_row_count_mismatch` until the set is complete; scope image QC to an intersecting production set (leftover completes skipped); TIFF magic before JPEG/PNG MIME; path-only `.jpg`/`.png` without magic or MIME is not image-eligible. Schema stays **41**. **0122–0126** product code not implemented. 0114 burn compose, 0115 G4 wrap, 0119 `volume_succeeded`, and 0120 overlay/Burn recount unchanged.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Resume OPT | PASS | OPT Error only when set status is `complete` / `complete_with_errors`. Test `run_production_qc_image_pack_skips_opt_until_complete` writes matching TIFF/SHA and asserts `r.passed` for `partial` / `running` / `failed`. Owner HITL remaining (cancel mid-image produce). |
| DoD-2 Completed OPT | PASS | `run_production_qc_image_pack_missing_opt_on_completed_volume` still fails. `run_production_qc_image_pack_missing_opt_explicit_set_id_still_errors` Errors with explicit `production_set_id`. |
| DoD-3 Leftover / moved | PASS | Unset heuristic skips when ≥2 complete image sets. Disk TIFF checks require `is_dir` + `read_dir`. Test `run_production_qc_image_pack_skips_leftover_complete_missing_root`. Chrome picker omits missing-root completes. Owner HITL remaining (moved `output_root`). |
| DoD-4 JPEG path | PASS | `sniff_kind` treats empty/whitespace MIME as absent; path-only `.jpg`/`.png` is Other. Tests: `path_only_jpg_without_magic_or_mime_is_not_eligible`, QC `run_production_qc_image_pack_path_only_jpg_not_missing`, produce `path_only_jpg_does_not_fail_closed`. Owner HITL remaining (garbage `.jpg`). |
| DoD-5 TIFF magic | PASS | Order PDF → TIFF magic → JPEG/PNG magic → path → MIME. `sniff_tiff_magic_beats_jpeg_mime_multi_ifd` (IFD count 2). Owner HITL remaining (multi-IFD TIFF tagged `image/jpeg`). |
| DoD-6 Hygiene | PASS | No new production `unwrap`/`expect`. Schema 41. Unused `looks_like_*_magic` helpers deleted. Chrome `pick_qc_production_set_id_omits_non_intersecting_partial`. 0119 latch + 0120 `frame_css_point` tests still pass. |
| DoD-7 Recorded | PASS | This file; registry **Completed**; `D-0121-image-opt-qc` closed. Ledger BUGFIX `381b22ad` committed on the product squash. |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test -p matter-qc` | pass (33 integration) |
| `cargo test -p pdf-raster` | pass |
| `cargo test -p matter-produce --test integration path_only_jpg_does_not_fail_closed` | pass |
| `cargo test -p dedupe-chrome pick_qc_production_set_id_omits_non_intersecting_partial` | pass |
| `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml latch` | 4 passed (0119) |
| `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml frame_css_point` | 2 passed (0120) |
| `cargo test --workspace` | pass |
| `ledgerful verify` | pass |
| CI (PR **#135**) | fmt, clippy, test, audit, deny, chrome-ui, verify-parity **green**. Bugbot skipping (does not block). |
| Codex r2 | **PASS**, no findings |

## Reviewer rounds

1. Internal: DoD-1…6 wired; schema / 0114–0120 fences held. **PASS** (no >low).
2. Codex r1: FAIL — P2 unreadable-root `read_dir`, resume test did not assert `passed`, empty MIME treated as present.
3. Codex r2: **PASS**. Fresh pass after those three fixes; prior P2s verified gone; no open >low.

## HITL (owner)

Release chrome EXE, synthetic image-profile matter: (1) cancel an in-flight image produce (`partial` / `running` / `failed` with pages and no `IMAGE.opt`) → QC / Finalize must not Error `opt_row_count_mismatch`; (2) complete volume, move/rename `output_root`, new Finalize of overlapping ids must not Error `image_page_missing` from the leftover; (3) a `.jpg` that is not JPEG magic ships native-only (no fail-closed); (4) a multi-IFD TIFF tagged `image/jpeg` produces one G4 page per IFD. INC* unique-pst is not a gate. Codesign is **D-0062-codesign**.

## Publish

- Branch: `track/0121-image-opt-qc-residuals`
- PR: **#135**
- Merge SHA: `600d6b368f183371b701ced1cbb138d8fae00be5`
