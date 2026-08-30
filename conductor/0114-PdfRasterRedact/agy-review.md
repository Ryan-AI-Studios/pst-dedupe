# Track review: 0114-PdfRasterRedact

**Harness:** Antigravity (`agy`)  
**Track:** `conductor/0114-PdfRasterRedact`  
**Date:** 2026-08-30  

## Summary

Line-by-line static audit of `0114-PdfRasterRedact` (`spec.md` + `plan.md`) against live workspace crates (`matter-core` schema v39, `matter-produce`, `matter-qc`, `extract-pdf`, `dedupe-chrome`), merged PRs (#115, #116, #117, #118), and `docs/deferred.md`.

The plan establishes a solid foundation for pure-Rust CPU page rasterization and true content-stream redaction using `zpdf` 0.13.x (MIT) without dragging in heavy GPL runtimes (MuPDF/pdf.js). It enforces a strict separation between draft UI overlays and burned CAS natives, and bumps the schema to v40. Adversarial review identified key technical blind spots: PDF coordinate mapping under non-zero CropBox/Rotate attributes, the necessity of a true non-incremental rewrite pass to eliminate trailing content streams, engine-level QC integration in `matter-qc`, and column ordering hygiene in `ITEM_COLUMNS`.

## Blind-spot headlines

1. **PDF CropBox and `/Rotate` coordinate skew:** Screen-to-PDF coordinate mapping will misplace redaction boxes on rotated pages or pages with non-zero CropBox/MediaBox origins.
2. **True non-incremental rewrite guarantee:** `IncrementalWriter` appends revision trailers; `pdf-raster` must enforce a complete rewrite/garbage-collection pass so previous content streams are excised from the file bytes.
3. **Engine-level QC rule for burned natives:** `burned_native_missing` must be formalized as a core rule in `matter-qc` alongside `redacted_text_missing` to guard all execution paths.
4. **`ITEM_COLUMNS` positional index stability:** New schema v40 columns must be appended to the end of `ITEM_COLUMNS` to avoid shifting 35+ existing positional `row.get(N)` mappings.
5. **Stale raster race condition:** Asynchronous raster generation across document navigation requires strict generation tracking to prevent painting previous documents.

---

## Findings (B/M/m/O)

| ID | Sev | Finding with concrete failure scenario | Fix |
|---|---|---|---|
| **M1** | Major | **PDF `/Rotate` and CropBox origin skew.** In §3.2.1 and §3.3, screen coordinate conversion assumes standard `(0, 0, W, H)` unrotated user space. In real-world PDFs with `/Rotate 90/180/270` or non-zero CropBox origins (e.g. `[36, 36, 576, 756]`), coordinates drawn on the rendered raster will map to the wrong physical area in the content stream, burning unrelated content and leaking sensitive text. | Return page `/Rotate` and `CropBox`/`MediaBox` bounds in `review_raster_page`; apply the inverse affine transform in the UI / host before persisting `item_geom_redactions` rects. |
| **M2** | Major | **True non-incremental rewrite enforcement.** In §2.4 and §3.6, the spec forbids incremental-only output. If `zpdf`’s `IncrementalWriter` outputs an incremental update by default, the original content stream remains accessible in the byte tail. | Ensure `pdf-raster` executes a full document rewrite (via `zpdf` full rewrite API or deserializing/reserializing via `lopdf`) so that superseded content streams and orphaned xref tables are completely eliminated from the burned CAS native. |
| **M3** | Major | **Engine-level QC gate for burned natives in `matter-qc`.** In §3.8, `burned_native_missing` is described as a chrome-level extra. If an operator triggers produce via CLI or script without the chrome wrapper, a missing burned native would only be caught during `resolve_native` mid-export rather than during pre-flight QC. | Add `RULE_BURNED_NATIVE_MISSING` (`burned_native_missing`) directly to `matter-qc::rules`, failing with Error severity when `(geom_redaction_count > 0 || (is_pdf && redaction_count > 0)) && burned_native_sha256.is_none()`. |
| **m1** | minor | **`ITEM_COLUMNS` index shift hazard.** In `crates/matter-core/src/matter.rs`, `ITEM_COLUMNS` and `map_item_row` use positional indices `0..105`. Inserting columns in the middle (e.g. after `redacted_source_digest` at index 69) will displace all subsequent field indices. | Append the five schema v40 columns (`geom_redaction_count`, `burned_native_sha256`, `burned_native_at`, `burned_source_digest`, `raster_engine`) at the end of `ITEM_COLUMNS`, leaving indices 0..105 intact. |
| **m2** | minor | **Stale raster race condition on queue switch.** When a user rapidly presses `]` to cycle documents, slow raster rendering for document A can complete after document B has loaded, overwriting document B’s view with document A’s raster. | Pass a monotonic `request_generation` counter and `item_id` in `review_raster_page`; discard responses in the UI if the generation does not match the active document. |
| **O1** | Observational | **Redact-from-hits bounding box padding.** Exact glyph bounding boxes returned from text search spans may be tightly bounded, risking letter ascender/descender bleed. | Apply a small 1-point margin dilation to search span bounding boxes when creating `source=hit` geometric redactions. |

---

## What looks solid

- **True Content-Stream Redaction:** Clean separation between draft UI overlays and burned CAS natives; strictly forbids black annotation overlays or incremental tail updates.
- **Fail-Closed Produce Defense:** `resolve_native` refuses to fall back to `native_sha256` when redactions exist on PDF or image items, preventing silent native leakage.
- **Pure-Rust MIT Footprint:** Clean integration of `zpdf` 0.13.x CPU rasterizer without GPL entanglements (MuPDF/Poppler/GPL pdfium).
- **Workspace Isolation:** Places PDF raster and burn logic in dedicated `pdf-raster` crate, keeping `ui/` wasm build clean and `extract-pdf` text-focused.

---

## Deferred fold-in table

| Deferred row (date / gist) | Spec disposition | Verified |
|---|---|---|
| `D-0032-01` (PDF geometric burn + image paint-burn) | **Absorb / close** for PDF content-stream + JPEG/PNG; TIFF G4 remains **0115** | ✓ Verified |
| `D-0034-02` (page raster preview) | **Absorb / close** (visible page + prev/next) | ✓ Verified |
| `D-0034-04` (geometric burn-in) | **Close** as duplicate of `D-0032-01` | ✓ Verified |
| `D-0030-01` (Image/PDF box markups) | **Partial** — geometric boxes added; text path already in 0032 | ✓ Verified |
| `D-0026-03` (HTML/image body) | **Partial** — Image tab rasterized; text/HTML already in 0112 | ✓ Verified |
| `D-0032-07` (full-page redact) | **Partial** — `source=full_page` in scope; inverse redact deferred | ✓ Verified |
| `D-0032-11` (redact all instances) | **Partial** — redact from hits covers quote instances on page | ✓ Verified |
| `D-0032-10` (MuPDF / redactor) | **Decline** — GPL license; zpdf MIT instead | ✓ Verified |
| `D-0034-03` (required pdfium bundle) | **Decline** — optional sidecar residual `D-0114-pdfium-sidecar` | ✓ Verified |
| `D-0034-05` (full interactive PDF viewer) | **Decline** — page nav only; full viewer deferred | ✓ Verified |
| `D-0034-06` (password bypass) | **Never** — encrypted PDFs fail closed | ✓ Verified |
| `D-0032-02` (Office native redact) | **Decline** — out of scope | ✓ Verified |
| `D-0036-08` (OCR after burn) | **Decline** — out of scope | ✓ Verified |
| `D-0040-01` / `D-0060-04` (TIFF/OPT image factory) | **Remain parked** — track **0115** | ✓ Verified |
| `D-0119-produce-checklist-residuals` | **Minted** from PR #117 Bugbot | ✓ Verified |

---

## PR / review comments the plan missed

- **PR #117 (Track 0113 Produce Checklist):** Bugbot reported Finalize re-arm on success, empty privilege filter export, and QC state surviving matter change. Correctly recognized and minted as **0119-ProduceWizardStateResiduals** without scope creep in 0114.
- **PR #115 (Track 0112 Review Window):** Stale fetch race conditions in review window are mirrored as a design consideration in 0114’s `review_raster_page` command (m2).

---

## Research / tools notes

- **ai-brains:** Checked preflight (3,983 pinned memories); verified decision `a9e41665-db69-4eb8-a0c8-998139f8a0ad` (zpdf 0.13.x MIT CPU raster, schema v40 `item_geom_redactions`, content-stream burn + rewrite).
- **ledgerful:** Ran `doctor` and checked ledger status (`0 pending / 0 unaudited drift`). Planning tx `3f43e5c4-7fb0-4adb-aa47-b1524d870a02` recorded.
- **online pins / crates.io:** Verified `zpdf` 0.13.x API shape (`CpuRenderer`, `IncrementalWriter::redact_page`), `image` crate PNG encoding, and leptos 0.8 CSR boundaries.

---

## Verdict: Ready after fixes

The plan is thorough and ready for execution once the findings (notably **M1** PDF rotation/CropBox transforms, **M2** non-incremental rewrite enforcement, and **M3** `matter-qc` rule addition) are folded into `spec.md` and `plan.md`.
