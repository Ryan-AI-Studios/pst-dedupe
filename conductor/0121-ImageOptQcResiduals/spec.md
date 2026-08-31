# 0121 — Image OPT / QC residuals (PR #121 Bugbot)

> Placeholder minted 2026-08-31 while expanding **0116**. Do **not** steal into
> Process fold. Expand with `/plan-track 121` before Implement.

- **Track ID:** 0121-ImageOptQcResiduals
- **Status:** Proposed — placeholder
- **Series:** O
>
> Four **valid** Cursor Bugbot findings on PR **#121** (`19d0c1f`) in
> `crates/matter-qc/src/rules.rs` and `crates/pdf-raster` / `matter-produce`.
> Not chrome Process (**0116**), not 0119 wizard Finalize UX, not 0120 Image-tab
> mouse/Burn counts.

## 1. Objective

Keep **0115** image produce + `qc_image_opt_v1` honest on resume and on mixed
volumes: missing `IMAGE.opt` must not block resume when pages are already
persisted; QC must not fail a new Finalize because an old volume folder moved;
JPEG/PNG path vs `sniff_kind` vs TIFF magic must agree so native-only items
are not fail-closed after the fact.

## 2. In scope (sketch)

PR #121 Bugbot (live-verified 2026-08-31 on HEAD `c6fb70c` / product `19d0c1f`):

1. **High — QC OPT check blocks resume** —
   `opt_row_count_mismatch` treats a missing `IMAGE.opt` as zero lines even
   when `production_image_pages` exist. Chrome Finalize always runs this pack
   before produce, so an interrupted image job cannot resume.
2. **High — QC scans every image volume** —
   Image QC walks every `production_sets` row, not the current job. A leftover
   or moved export folder Errors `image_page_missing` on overlapping ids and
   blocks a new Finalize.
3. **Medium — JPEG path eligibility mismatches pages** —
   `is_image_eligible_native` treats `.jpg` / `.jpeg` / `.png` as eligible when
   `sniff_kind` is Other and `native_image_page_count` is 0. Produce ships
   native-only, then `check_image_fail_closed` fails the volume.
4. **Medium — MIME wins over TIFF magic** —
   `sniff_kind` returns JPEG/PNG from MIME before TIFF magic or a `.tif` path,
   so multi-IFD TIFF tagged `image/jpeg` never becomes G4 pages.

## 3. Out of scope

Process fold (**0116**), queue (**0117**), window async (**0118**), produce
wizard Finalize/filter_ids (**0119**), Image-tab overlay coords (**0120**),
LFP/colour/email-print (**D-0115-***).

## 4. DoD (sketch)

- [ ] Missing OPT with persisted pages does not Error-block resume (Warn or skip until write).
- [ ] Image QC scoped to the **current** production set / job, not leftover volumes.
- [ ] Path-only JPEG/PNG with zero pages is native-only, not fail-closed.
- [ ] TIFF magic / `.tif` wins over a lying JPEG MIME.
- [ ] `review.md`; registry Completed; ledger committed.

## 5. Notes

Next free ID after this mint: **0122**. No BCC track.
