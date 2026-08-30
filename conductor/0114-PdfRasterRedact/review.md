# 0114-PdfRasterRedact — Review

- **Track:** `0114-PdfRasterRedact`
- **Branch:** `track/0114-pdf-raster-redact` (product) → `docs/0114-completed` (registry)
- **Registry:** **Completed**
- **Product PR:** **#119** squash-merged to `main` as `5ed53bf`
- **Docs PR:** records this Completed registry (after product merge)

---

## Definition of Done (§7)

| DoD | Status | Evidence |
|---|---|---|
| **DoD-1 — Image tab rasters** | **PASS** (engineering) | zpdf 0.13.0 CPU PNG raster on the Image tab; page 2 via `,`/`.`; JPEG/PNG decoded pixels; EML `unsupported_kind` (“Not a page image (TIFF/OPT is 0115).”); encrypted PDF honest `encrypted` / `pdf_encrypted`; `/Rotate 90` host map (`rotate90_visible_token_burns_via_host_upsert`); stub copy `"No raster yet (0114)."` gone. `dedupe-desk` still in workspace. No `process-runner` on `dedupe-chrome`. No zpdf in `ui/`. Owner release-EXE HITL residual (below). |
| **DoD-2 — Draft ≠ burned** | **PASS** | Geom create does not change `native_sha256`. Overlay list/JSON present. Produce without Burn fail-closes `burned_native_missing` when geom count > 0. Highlights never become geom rows. |
| **DoD-3 — True burn** | **PASS** | `burned_native_sha256` ≠ `native_sha256`; token absent UTF-8 and UTF-16LE; extract of burned file has no token; original CAS still does; `resolve_native` writes burned bytes; compose is `redact_page` → incremental `write` → `PdfFile::parse` → `rewrite_pdf` (`iw.document()` forbidden). Second 0032 text redaction without geom fail-closes until re-burn (`quote_unmapped` + fingerprint includes text count). JPEG burn keeps JPEG magic / `FILE_EXT=jpg`. |
| **DoD-4 — Text-redact-on-PDF honesty** | **PASS** | Active 0032 text redaction + zero geom → QC Error `text_redact_unmapped_on_pdf`. From-hits creates `source=hit` boxes. Zero `search_spans` hits is unmapped, not a silent skip. |
| **DoD-5 — Schema + CI** | **PASS** | `SCHEMA_VERSION` **40**. v40 cols 155–159 appended after `teams_extract_error` (`row.get(154)` unchanged). Geom CRUD + fingerprint + `burned_native_missing` covered. New `allow-*` for raster/geom/burn. Encrypted → `encrypted`. PR **#119** required CI green (fmt/clippy/test/audit/deny/chrome-ui/verify-parity). `cargo deny` accepts zpdf MIT. No production `unwrap`/`expect`. CSP `img-src 'self' data:` unchanged (PNG data URLs only). |
| **DoD-6 — Recorded** | **PASS** | Product PR **#119** / `5ed53bf`. Registry **Completed**; `D-0032-01` / `D-0034-02` closed (TIFF half of D-0032-01 → **0115**); `D-0034-04` closed as duplicate. **0115** parked. **0116** / **0117** / **0118** / **0119** stay Proposed. |

---

## Internal review rounds

| Round | Open | Outcome |
|---|---|---|
| 1 | CSS/raster mapping, Image-tab Burn, generation discard, JPEG y-flip, full-page MediaBox, from-hits all quotes | Fixed before Codex r1 |

---

## Codex completion audits

| Round | Verdict | Notes |
|---|---|---|
| r1 | **FAIL** (rotate 90/270 vs zpdf clockwise; metadata PDF gate; `set_burned_native` any digest; Burn default `ordered_ids`; DoD-6 OOS) | Fixed |
| r2 | **FAIL** (unbound snapshot; capped JPEG/PNG coords; default-set fallback; prefix sniff; truncation banner; keyboard-delete generation) | Fixed |
| r3 | **FAIL** (P1 existing geom certified new text redaction B; P2 client raster dims; P2 Burn counters on default set) | Fixed (`quote_unmapped`; reject dim mismatch; QC carries need_burn) |
| r4 | **FAIL** (silent geom upsert; EML generic error; `pdf_raster_failed` extra unwired) | Fixed |
| r5 | **FAIL** (PDF cap omitted `truncated`; rotation not through chrome host; extras as `card blocker`) | Fixed |
| r6 | **FAIL** (LRU `CacheEntry` omitted `truncated`; cache hits hard-coded false) | Fixed |
| r7 | **FAIL** (host still trusted client raster dims) | Fixed (`host_raster_dims` reject >0.5 px) |
| r8 | **FAIL** (`selected_geom` not cleared on doc/page change; `cargo fmt`) | Fixed |
| **r9** | **PASS** | `review.codex.r9.md` — no open P0–P3 |

**Open engineering findings > low:** none.

---

## Residuals (deferred / external)

| Item | Notes |
|---|---|
| **Owner HITL** | Release EXE + synthetic PDF whose stream contains `SECRET_TOKEN_0114`: draw box, Burn, Finalize DAT, confirm produced native does not contain the token. INC* waived. |
| **D-0114-pdfium-sidecar** | `pdfium-fallback` did **not** ship. zpdf CPU is the required path. Optional `pdfium.dll` next to the EXE remains residual. |
| **D-0114-xform-text** | zpdf does not descend Form XObjects; intersecting placement drops the whole XObject. Nested form-text under-redact residual. |
| **D-0032-01 TIFF half** | PDF + jpeg/png closed here. TIFF G4 / OPT stays **0115**. |
| **D-0032-07** | `source=full_page` shipped. Inverse redact declined / residual. |
| **D-0032-11** | From-hits all spans of the quote shipped. Manual multi-string still residual. |
| **D-0034-05** | Full Acrobat-class viewer (thumbs/outline/forms) remains. This track is visible page + prev/next. |
| **D-0026-03** | Image tab raster closed here. HTML browser engine residual. |
| **D-0115** / **D-0116** / **D-0117** / **D-0118** / **D-0119** | Stay as they are. |
| **D-0110-deny-unic** | Remains (upstream unic-* via Tauri). |
| **D-0062-codesign** | Release ops; not this track. |

---

## Pins / stack (re-verified at implement)

- `zpdf` **0.13.0** MIT; feature `cpu-render` default; **no** `gpu-render`
- `tauri` **2.x**; `leptos` **0.8** CSR; `trunk` **0.21.14** (CI `chrome-ui`)
- `SCHEMA_VERSION` **40**
- Ledger FEATURE tx `16da6240-9930-4474-8b8f-5e04ff728f4e` (hook-promoted on product commit `a87802e` / merge `5ed53bf`)
- Ledger DOCS tx `0d7b5511-39ff-4b5f-a9d1-6801510bceeb` (this Completed registry)

---

## Conductor files requiring `git add -f`

```
conductor/0114-PdfRasterRedact/spec.md
conductor/0114-PdfRasterRedact/plan.md
conductor/0114-PdfRasterRedact/review.md
conductor/0114-PdfRasterRedact/review.codex.r1.md
conductor/0114-PdfRasterRedact/review.codex.r2.md
conductor/0114-PdfRasterRedact/review.codex.r3.md
conductor/0114-PdfRasterRedact/review.codex.r4.md
conductor/0114-PdfRasterRedact/review.codex.r5.md
conductor/0114-PdfRasterRedact/review.codex.r6.md
conductor/0114-PdfRasterRedact/review.codex.r7.md
conductor/0114-PdfRasterRedact/review.codex.r8.md
conductor/0114-PdfRasterRedact/review.codex.r9.md
conductor/0114-PdfRasterRedact/foldin-note.md
conductor/0114-PdfRasterRedact/opencode-review.md
conductor/0114-PdfRasterRedact/agy-review.md
conductor/conductor.md
conductor/ROADMAP.md
conductor/sequencing.md
```
