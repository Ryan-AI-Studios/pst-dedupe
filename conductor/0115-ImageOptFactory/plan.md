# 0115 — ImageOptFactory — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> **Ledger (implement):** `ledgerful ledger start crates/matter-produce --category FEATURE --message "0115 TIFF G4 + Opticon OPT factory"` — commit in the final phase.
>
> Planning tx (Ready pass): `805b9cf0-8155-49a1-b158-bf4b7e42665d`.
> Fold-in tx: `6ccb6e4d-cb07-42b1-be44-e6a5ffe1f39c` (`opencode-review.md` + `agy-review.md`; spec §2.10).

---

## Phase 0 — Precondition / pin gate → DoD-1, DoD-5

- [ ] Re-verify `SCHEMA_VERSION` **40**, `production_items` has no `end_bates` / `page_count`, `VolumeLayout` has no `IMAGES`, DAT `BEGBATES=ENDBATES`, chrome Format/Number copy names 0115, `pdf-raster::NativeKind` has no Tiff (spec §2.2).
- [ ] Re-verify crates.io `fax` **0.3.x** MIT. Confirm:
  - `Encoder::new(VecWriter::new())` + `encode_line` + `finish()` (two `EDFB_HALF`)
  - `fax::tiff::wrap` still exists **and still must not be the produce path** (X/YRes=200, no BitsPerSample)
- [ ] If G4 bitstream encode is unavailable without a denied license, **stop**.
- [ ] Re-verify tauri **2.x** + leptos **0.8**. Keep `ui/` workspace **exclude**. Keep `chrome-ui`.
- [ ] Do **not** vendor `C:\dev\dedupe-frontend`. Do **not** add ImageMagick/Ghostscript/MuPDF/Poppler/GPL `pdfium`. Do **not** implement **0116** / **0117** / **0118** / **0119** / **0120**.
- [ ] Do **not** add `process-runner` to chrome. Do **not** depend on `dedupe-desk`.
- [ ] Confirm DAT-only default profile still has no `include_images`.

## Phase 1 — Schema v41 + G4 engine → DoD-2, DoD-5

- [ ] `SCHEMA_VERSION = 41`. ALTER `production_items` add `end_bates`, `page_count`. New table `production_image_pages` (spec §3.2). **Do not** append `ITEM_COLUMNS`.
- [ ] Profile body additive fields: `packaging.include_images` (default false), `bates.mode` (`document`\|`page`), `layout.images`, `packaging.image_dpi=300`, `image_folder_cap=500` with **validation `image_folder_cap ≥ MAX_PAGES`**. Builtin **`us_concordance_image_opt_v1`** reserved; bound pack **`qc_image_opt_v1`**.
- [ ] `pdf-raster`: `NativeKind::Tiff` (II*/MM* magic); inbound TIFF **decode each IFD**; `DPI_PRODUCE=300` for PDF; BT.601 luma + threshold 160; Bates endorse (solid box, ≥0.25 in) **before** threshold; `fax` `Encoder`/`encode_line`/`VecWriter`/`finish`; **explicit little-endian IFD** (spec §2.5 tag list). `catch_unwind` at document boundary. Enable `image` `tiff` feature for **decode only**.
- [ ] Tests: oracle Compression=4 **and** BitsPerSample=1 **and** XRes=300 (wrap output must fail this); 2-page PDF → two single-IFD TIFFs; inbound **2-IFD TIFF → two G4 files** (original not copied); JPEG → one G4 page.

## Phase 2 — Produce + OPT + QC → DoD-1, DoD-2, DoD-3, DoD-4

- [ ] `VolumeLayout` grows `images` when `include_images`. DAT-only create path **must not** mkdir `IMAGES`.
- [ ] Page-level `next_seq` (spec §3.4). **Change** `advance_next_seq_for_control` to honor **end_bates** (live code is beg-only). Unique `(set, bates)` on image pages. **Crash-mid-document** test: 3-page item, interrupt after page 1, resume, next beg = old end+1.
- [ ] OPT writer (`IMAGE.opt`, CRLF, 7 fields, `Y` + count on first page only). Native-only items omitted from OPT. Volume field: strip comma + whitespace (not only `sanitize_filename_part`).
- [ ] DAT `ENDBATES` from `end_bates`. `CONTROL_NUMBER` = beg. Natives still named by **BEGBATES**.
- [ ] Raster **the same bytes** `resolve_native` would ship (burned if required). Missing burn → existing `burned_native_missing`.
- [ ] Folder shard: `image_folder_cap`; never split a document; `page_count > cap` → item error.
- [ ] `matter-qc`: pack `qc_image_opt_v1` = default + Warn `image_skipped_native_only` + Error `beg_end_bates_span` / `multi_page_tiff_as_artifact`. Span/OPT oracles read **persisted** `production_items` / `production_image_pages`. Produce-internal fail-closed if image-eligible item writes zero TIFFs or OPT/DAT span mismatch. **Do not** change `qc_default_v1`.
- [ ] Tests: DoD-1 DAT-only (no IMAGES); DoD-2 2-page PDF image volume + inbound 2-IFD; DoD-3 xlsx/eml native-only; DoD-4 token absent from G4 after burn.

## Phase 3 — Chrome Format/Number + TIFF Image tab → DoD-2, DoD-5

- [ ] Format: builtin image profile selectable; drop “ships in 0115” copy when that profile is on; copy names native-only types + no LFP.
- [ ] Number: page-level Bates enabled **only** with image profile (selecting page-level selects the image profile; DAT-only forces document-level). Family-together stays locked.
- [ ] Image tab: TIFF decode 1:1 + `LONG_SIDE_CAP` (not `DPI_REVIEW`) + page nav. EML empty stays native-only honest. **Do not** touch overlay mouseup / drawing-clear / Burn-count snapshot (**0120**).
- [ ] Commands stay `join_worker`. Autogenerate `allow-*`. Encrypted root → `encrypted`.
- [ ] Do **not** fix 0119 Finalize re-arm.

## Phase 4 — CI + docs → DoD-5

- [ ] `cargo fmt` / `clippy -D warnings` / `cargo test --workspace` / crate tests in spec §8 / `cargo check -p dedupe-desk` / trunk `ui/`.
- [ ] Assert `crates/dedupe-chrome/ui/Cargo.toml` has no `fax` / `tiff` / `zpdf`.
- [ ] `cargo deny` (`fax` MIT). Record pin in CHANGELOG.
- [ ] CHANGELOG Unreleased. Close `D-0040-01` / `D-0060-04` (OPT half) per spec §9. Residuals `D-0115-lfp` / `D-0115-color` / `D-0115-email-print` already in deferred. Leave **0116–0120** as-is.

## Phase 5 — Finalize → DoD-6

- [ ] Owner HITL: **release** EXE. Image profile, 2-page synthetic PDF + spreadsheet, Finalize, OPT/TIF/DAT match. INC* waived.
- [ ] `review.md`; `../conductor.md` + `sequencing.md` + `ROADMAP.md`: **0115 Completed**.
- [ ] Commit the ledger transaction.
- [ ] **0116** / **0117** / **0118** / **0119** / **0120** still Proposed.

---

## Handoff notes

- Image produce is outward-facing: a DAT span that does not match OPT page count is a **defect**, not a known_gap. A TIF whose XRes tag is 200 while pixels were rastered at 300 is also a **defect**.
- Rollback: unused TIFFs under `exports/productions/` are operator-local (gitignored via `output/` / matter exports). Original natives remain. DAT-only profile remains the default.
- Single-exe / no-daemon remains. G4 is CPU; never on the WebView thread.
- Re-verify `fax` APIs at execute — method names are **plan-time 0.3.0**. `wrap` remains forbidden even if still present.
- Execute only when the user says **Implement**.
