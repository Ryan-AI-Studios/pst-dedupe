# Track review: 0114-PdfRasterRedact

**Harness:** OpenCode (`opencode`)
**Track:** `conductor/0114-PdfRasterRedact`
**Date:** 2026-08-30
**Mode:** review only — no implement, no fold.

## Summary

Nearly every §2.2 pin verifies exact against live workspace at HEAD `6ca24be`:
`SCHEMA_VERSION == 39` (`matter-core/src/schema.rs:11`); `Item.redaction_count` at
`matter.rs:257` (spec's "~256–262"), `ITEM_SELECT` redaction columns at `matter.rs:6157`
(spec's "~6157" — exact hit) with `row.get(66)` at `:6255` (the shift hazard is real);
v13 pattern `ALTER TABLE items ADD COLUMN redacted_text_sha256` at `schema.rs:513`;
`item_redactions` is the 0032 char-range table (reason/label/status live, no rect columns —
v13 verify run at `schema.rs:2882`); extract-pdf caps `MAX_NATIVE_INPUT_BYTES = 100 MiB` +
`MAX_PAGES = 500` (`extract-pdf/src/limits.rs:4,7`); `resolve_native` copies
`native_sha256` unconditionally (`matter-produce/src/resolve.rs:205-229`) while
`resolve_text` fail-closes via `redaction_count > 0` (`resolve.rs:273-274`); stub copy
`"No raster yet (0114)."` at `ui/src/pages/review_window.rs:695`, overlay `"r — Image tab
(0114 stub)"` at `:577`, Burn copy `"Only CAS redacted text is packaged. Geometric PDF burn
is 0114. Highlights never burn."` at `ui/src/pages/produce.rs:367`; CSP `img-src: 'self'
data:` (`tauri.conf.json:31`); fixtures `fixtures/pdf/{corrupt,empty,low_text,minimal}.pdf`
all present; `join_worker` at `dedupe-chrome/src/lib.rs:49` and no `process-runner` in the
chrome crate (grep clean); trunk 0.21.14 at `ci.yml:106`. The external crate pin is the
best-verified part of this plan: I downloaded and **read the zpdf 0.13.0 source from
crates.io** — released 2026-08-25, MIT (spec's pin date exact); features are
`cpu-render` (default, pulls `zpdf-render-cpu`) and opt-in `gpu-render` (GPU stays off by
default, satisfying the no-wgpu rule without config); `IncrementalWriter::redact_page(page_index, rects, options)` with
y-up user space, intersect annotation removal, and whole-XObject-drop-on-intersect is
verbatim (`zpdf-writer-0.13.0/src/redact.rs:29-57` — "Text inside Form XObjects is not
descended into (the whole XObject is dropped if its placement intersects)": the §6 XObject
risk row is a fact, not a guess); `RedactOptions { fill: Option<(f64,f64,f64)> }` defaults
to black (`redact.rs:29-40`); `rewrite_pdf(source: &PdfFile, options: &RewriteOptions)` GCs
orphans from incremental edits (`rewrite.rs:1-11,98`); `IncrementalWriter::new` refuses
encrypted docs (`lib.rs:124` + module docs); `search_spans(&[TextSpan], query,
case_sensitive) -> Vec<SearchHit>` with y-up `rects` per matched line
(`zpdf-content-0.13.0/src/search.rs:17-28,73`); `Rect{x0,y0,x1,y1}` (`zpdf-core geometry.rs:26-31`);
`CpuRenderer` implements `RenderBackend::render_display_list(dl, scale)` → `RenderedPage`
and carries its own anti-hang budgets (`max_page_pixels`, clip-pixel budget, wall-clock
render deadline — hostile-PDF mitigation exists at engine level, `zpdf-render-cpu`
lib.rs). PRs #118/#117/#116/#115 match; #117 Bugbot is exactly the three minted 0119 items
(verified live: `produce.rs` ui :492 High Finalize-armed, :66 QC-state-across-matters,
host :867 empty-filter log dump), #118 has zero inline comments; deferred rows
D-0032-01/0034-02/0034-04/0030-01/0026-03/0032-07/11/10/0034-03/05/06/0032-02/0036-08 +
0117/0118/0119 all live with matching dispositions (`deferred.md:118,161,191-231,267,887,
897,919,924-935`) and next-free-ID is **0120** (`:935`). **Worst miss in one sentence:**
the burn freshness fingerprint omits text-redaction state, so a 0032 redaction added
*after* a successful burn leaves `burned_source_digest` "fresh" and produce ships a burned
native that still contains the newly redacted words.

## Findings (B/M/m/O)

- **B — none.**

- **M1 — `burned_source_digest` excludes text-redaction state → post-burn 0032 redaction
  ships in the produced native.** §3.2.2 pins the fingerprint as `native_sha256` + canonical
  active **geom** list + engine pin, and §3.6's burn-required gate re-burns when the
  fingerprint mismatches. Native change → stale (covered); drawn-geom change → stale
  (covered, geom list changes). But a **text** redaction via `create_redaction` (0032)
  changes neither: native same, geom list same → fingerprint same → burn reads "fresh" —
  while the new redacted text is still excised from *nothing*: the burn's `redact_page`
  only removed the *original* geom rects, and `text_redact_unmapped_on_pdf` fires only
  when there are **zero** geom rows (§3.5), which this item doesn't have. Concrete
  sequence: counsel draws a box on page 1 → Burn → Finalize delayed; later codes token-2
  as a 0032 text redaction → produce day: blocker count is clean, burn is fresh, the
  DAT native contains token-2. The exact defect class §2.6 calls "fails the track,"
  reachable through the track's own design. Planner fix (one line): fold
  `redacted_text_sha256` (or a count+max(updated_at) fingerprint of active 0032 rows)
  into `burned_source_digest` **and** into the burn-required predicate — any text-redact
  change → stale → re-burn. Add a DoD-3 variant: burn, then add a text redaction, then
  produce → must refuse/re-burn, not ship.

- **M2 — The §3.6 burn recipe does not compose as written; the required
  serialize→reparse round-trip is unnamed and the obvious literal reading compiles but
  burns the original.** `rewrite_pdf` takes `&PdfFile` (`rewrite.rs:98`), while
  `redact_page` edits live in `IncrementalWriter`'s `pending: BTreeMap` overlay —
  `iw.document()` returns only the **base** document and its own doc comment warns
  "pending edits are *not* [visible]" (`zpdf-writer lib.rs:105,184-188`). So
  `rewrite_pdf(iw.document(), …)` **compiles** and garbage-collects the *original*
  object graph — token intact — while the redacted page dict sits in pending, never
  emitted. DoD-3's byte-oracle would fail this at test time (good), but the plan should
  not leave a compiling wrong path as the first thing an implementer tries. Planner fix
  (one line): pin the burn pipeline as
  `IncrementalWriter::new(original)` → `redact_page`×N → `iw.write(&mut Vec<Cursor>)` →
  `PdfFile::parse(cursor.into_inner(), limits)` → `rewrite_pdf(&parsed, &RewriteOptions)`
  → `put_bytes`, in §3.6 + Phase 1 (the "(or the current full-rewrite API)" hedge in
  §2.4 stays, but the composition constraint is the normative part).

- **m1 — `render_display_list` does not return PNG; it returns raw RGBA.** `RenderedPage`
  is `{ width, height, data: Vec<u8> }`; the only PNG path is
  `save_png(path)` which **writes a file and consumes the page**
  (`zpdf-render-cpu lib.rs:1746-1754`). §2.4's "CpuRenderer::render_display_list(&list,
  150.0/72.0) → PNG" overclaims one step. The host command contract (§3.7) needs PNG
  *bytes* for CAS / data-URL; either name the extra encode (`image::RgbaImage::from_raw`
  + PNG encoder — `image` is already in the zpdf-render-cpu tree) or pin write-to-temp +
  read. Trivial, but it is an invented API shape as written.

- **m2 — JPEG paint-burn re-encodes to PNG but the produced filename keeps `.jpg`.**
  §3.6: jpeg natives are burned by painting black and "re-encode PNG into
  `burned_native_sha256`", while `resolve_native` derives the destination from the item's
  extension (`extension_from_item` → `NATIVES/<control>.jpg`, `resolve.rs:219-221`). A
  burned-august .jpg whose bytes are PNG is a load-file honesty defect (QC diff tools and
  strict consumers key on extension). Planner fix: either keep JPEG encoding for jpeg
  sources (quality note) or pin that the produced extension follows the *burned* payload
  (and where that override lives), not the source item.

- **m3 — The mandated stale-reply guard has no mechanism because generation does not
  exist yet.** §2.8/#115 requires new raster commands to ignore stale replies, and §3.7
  repeats it — but the current Image/pane fetches have **no epoch concept at all**
  (`review_window.rs:163-239` spawn and `set` on resolve unconditionally; that is the
  very #115 class, still open as 0118). "item_id + generation" therefore requires
  minting a generation source (per-tab Leptos signal bumped on doc change) and this
  track owning it for its new commands while 0118 owns the old ones — say so in Phase 2,
  or the implementer will look for an existing counter that isn't there.

- **m4 — `search_spans` never matches across line boundaries and returns one rect per
  matched line.** A multi-line `exact_quote` in from-hits finds 0 hits — correct
  fail-closed only if §3.5 requires an honest zero-hit report (and, when
  `redaction_count > 0`, the unmapped blocker fires). Pin that sentence; also note the
  per-line rects mean "all instances" granularity is line-accurate, fine for D-0032-11's
  slice.

- **m5 — The page → `DisplayList` construction route is not named.** The plan's phase-0
  confirm list checks `render_display_list` but not how a `PdfDocument` page yields a
  `DisplayList` (the interpreter/display-sink path — cf. the `dump_spans` example using
  `ContentInterpreter`). Add "DisplayList construction" to the Phase-0 confirm list so
  execute doesn't stall on an unplumbed seam. (Everything else on that list I have now
  verified live for 0.13.0.)

- **O1 — §8 says `cargo test -p matter-core` while DoD-5 says `cargo test -p
  matter-core --lib`.** Harmless; make them one command so CI parity is unambiguous.

- **O2 — DoD-1's encrypted-fixture hedge "(or a documented fail-closed stand-in)" can be
  strengthened: zpdf ships `EncryptionConfig` in the writer (`encrypt.rs:132`), so a real
  RC4/AES synthetic encrypted fixture is generatable in-repo. Foldin may upgrade the DoD
  wording; decline if it bloats the fixture story.

## What looks solid

- **The incremental-only trap is fenced on both sides:** §2.4's produce-burn lock
  ("IncrementalWriter::write alone is forbidden") matches the library's own module docs —
  the writer appends objects + `/Prev` trailer, "the original file content is left
  untouched," and `rewrite_pdf` exists precisely to drop "orphans from incremental edits"
  (`rewrite.rs:8`). DoD-3 makes the file-tail leak a hard fail with UTF-8 + UTF-16LE + 
  extract oracles. P2's "if rewrite is gone at execute, stop" pre-registers the one
  dependency that makes the promise keepable.
- **The XObject risk row is a verified fact, not folklore:** zpdf's redact.rs doc block
  states the exact over-redact/under-redact semantics the spec claims (whole XObject
  dropped when placement intersects; no descent). Residual D-0114-xform-text + "prefer
  fail-closed" is the right posture, and my composition reading (M2) doesn't change it.
- **Hostile-PDF mitigation is engine-native, not just wrapper-level:** CpuRenderer carries
  `max_page_pixels` (uniform downscale, complete-page guarantee), clip-pixel and blend
  budgets, a per-page wall-clock deadline ("legit pages finish in well under"), and
  soft-mask/depth guards against cyclic mask graphs; zpdf's parser ships full-file
  object-scan recovery and budgeted interpretation. The §6 hang/OOM row has real teeth
  under it; `ParseLimits` + caps + `catch_unwind` on top is belt-and-braces, honest.
- **No-GPU-by-default is structurally true:** `cpu-render` is the default feature and
  `gpu-render` is opt-in — the workspace only has to *not* enable it; description says
  "with wgpu GPU rendering" but the dep is optional (verified in the normalized
  Cargo.toml).
- **License/deny posture verified:** zpdf 0.13.0 is MIT (crates.io API, single license
  field), no GPL anywhere in its graph at the facade level (dev-deps p256/rsa/rand_chacha
  are test-only; rsa 0.9 is already dispositioned D-0062-audit-rsa upstream); deny.toml
  exists and the MIT pin slot is consistent.
- **Fail-closed chain text vs code matches:** burn-required when `geom_redaction_count > 0`
  OR (PDF ∧ `redaction_count > 0`) mirrors `resolve_text`'s live `redaction_count > 0`
  gate (`resolve.rs:273`); "never falls back to original native" matches today's
  unconditional copy (`resolve.rs:213-229`) — the change is a strict replacement, no
  fallback semantics to accidentally preserve.
- **Detect-surface claim verified:** `extract-pdf::detect` has `looks_like_pdf`,
  `is_pdf_eligible_meta`, `detect_pdf(path, mime, bytes)` with BOM/whitespace-tolerant
  magic sniff (`detect.rs:19-50`), and the plan's "path dep OK / copy usage" keeps
  `pdf-raster` off `dedupe-desk`.
- **Stub/copy targets all real:** the three 0114-named copy strings live exactly where
  §2.2 says (`review_window.rs:577,695`, `produce.rs:367`), so DoD-1's "copy is gone" is
  a greppable gate.
- **Governance/ledger surface verified:** planning tx `3f43e5c4` in the ledger (Docs /
  0114-PdfRasterRedact, 2026-08-30); ai-brains recall returns the 0114 Ready decision
  matching the spec's locks verbatim (redact→rewrite order, highlights-never-burn, no
  process-runner, 0119 mint); ledgerful doctor readyForPublish true, 0 pending / 0 drift;
  dirty tree is conductor/deferred edits + untracked scratch per the planning pass.

## Deferred fold-in table

| Deferred row (date / gist) | Spec disposition | Verified |
|---|---|---|
| **D-0032-01** geometric PDF/image burn (`:191`,`:926`) | **Absorb / close** (PDF stream + jpeg/png; TIFF→0115) | ✓ live, 0114 Ready |
| **D-0034-02** raster preview (`:227`,`:927`) | **Absorb / close** (page + nav; not D-0034-05) | ✓ |
| **D-0034-04** geometric burn-in dup (`:229`) | Close as duplicate of D-0032-01 | ✓ row exists |
| **D-0030-01** Image/PDF box markups (`:161`) | Partial (text 0032; geom here) | ✓ |
| **D-0026-03** HTML/image body (`:118`) | Partial (Image raster; HTML 0112) | ✓ |
| D-0032-07 inverse/full-page (`:197`) | Partial — `full_page` in; inverse residual | ✓ |
| D-0032-11 redact-all-instances (`:201`) | Partial — from-hits all spans | ✓ (per-line rects verified) |
| D-0032-10 MuPDF (`:200`) | Decline (license) | ✓ |
| D-0034-03 pdfium bundle (`:228`) | Decline as required; optional sidecar residual | ✓ |
| D-0034-05 full viewer (`:230`) / D-0034-06 password (`:231`) | Decline / never | ✓ |
| D-0032-02 Office native redact (`:192`,`:214`) | Decline | ✓ |
| D-0036-08 OCR of burned (`:267`) | Decline | ✓ |
| D-0040-01 / D-0060-04 TIFF/OPT | Remain 0115 parked | ✓ |
| D-0117 / D-0118 / D-0119 (`:933` area rows) | Remain Proposed; not stolen | ✓ rows + folders exist |
| D-0113-long-job (`:924`) | Remain / 0116 | ✓ |
| D-0110-deny-unic (`:919`) / D-0062-codesign (`:897`) / D-0108 (`:887`) | Remain / decline | ✓ |
| D-0114-pdfium-sidecar / D-0114-xform-text | Minted on Implement if unshipped | consistent — not yet rows |

No open med/high row overlaps the raster/burn surface that the spec misses. Next free ID
**0120** (`deferred.md:935`).

## Cursor / last-PR comments the plan missed

Last 4 merged verified via gh: **#118, #117, #116, #115** — matches §2.8. `gh api`: #117
has exactly the three Bugbot inline comments the spec dispositions and mints 0119
(`produce.rs` ui :492 / :66, host produce.rs :867) — none stolen into this track, correct.
#118 is docs-only with zero inline comments. #115's three items remain 0118 and this
track's "new commands need their own guard" note (§2.8) is the right inheritance — my m3
makes it concrete. No undispositioned comments; no new placeholder needed — **0120**
stands.

## Research / tools notes

- **ai-brains: used** from `C:\dev\Dedupe` — preflight inited, **3983** pinned (spec §2.5
  says 3981; +2 self-correcting drift, continuing pattern); `sync query` recovered the
  0114 Ready decision (`a9e41665`) matching the spec's locks verbatim (zpdf 0.13.x CPU in
  new pdf-raster crate, schema v40 `item_geom_redactions` + `burned_native_sha256`,
  redact_page THEN rewrite_pdf with "incremental write forbidden", original native never
  rewritten, highlights never burn, text-redact-on-PDF requires geom or fail-closed,
  jpeg/png paint-burn in, TIFF/OPT 0115, no process-runner, no pdf.js/MuPDF/GPL pdfium,
  pdfium sidecar optional residual, #117→0119); recall reconfirmed 0034 (text-only, no
  pure-Rust raster — the very gap this track closes) and 0026/0030/0032 lineage.
- **ledgerful: used** from `C:\dev\Dedupe` — doctor readyForPublish **true** (standing
  warns: phantom-promote legacy, sig-pin, sig-version, completion-unreachable); `ledger
  status --compact` **0 pending / 0 unaudited drift**; ledger search confirmed tx
  **`3f43e5c4`** (0114-PdfRasterRedact, Docs, 2026-08-30 14:54, "Ready spec/plan");
  `scan --impact` LOW (docs/conductor + deferred edits + untracked scratch, no product
  crates touched at review time; output/ file-budget warns are a known scanner nuisance,
  add `[federation] scan_exclusions`).
- **Online: verified at the source.** crates.io API: `zpdf` latest **0.13.0**, published
  **2026-08-25**, MIT, not yanked (spec pin exact); features `cpu-render` (default) /
  `gpu-render` (optional). I downloaded and read zpdf 0.13.0 plus sub-crates
  (`zpdf-writer`, `zpdf-render-cpu`, `zpdf-render`, `zpdf-content`, `zpdf-core`,
  `zpdf-parser`) from the registry: `IncrementalWriter::{new, new_with_password,
  write<W: Write+Seek>, redact_page}` with `RedactOptions{fill:Option<(f64,f64,f64)>}`
  default black; XObject no-descend doc verbatim; `rewrite_pdf(&PdfFile,
  &RewriteOptions)->Vec<u8>` with orphan GC; `RenderBackend::render_display_list(dl,
  scale) -> RenderedPage {width,height,data}` (+ `save_png(path)`); `CpuRenderer`
  anti-hang budgets; `search_spans` (per-line rects, never-cross-line matching);
  `PdfFile::parse_with_limits`; `EncryptionConfig` in writer. **M2's composition trap and
  m1's PNG-encode gap came out of reading that source** — docs.rs alone would have
  blessed the naive recipe. tauri 2.x / leptos 0.8 / trunk 0.21.14 re-verified in
  workspace + ci.yml (no churn since #117). MS-PST: N/A — confirmed nothing in this
  track touches PST parsing.

## Verdict: Ready after fixes

No B. Two M's, both one-line planner folds plus one DoD variant:

1. **M1** — add text-redaction state (`redacted_text_sha256` or active-text fingerprint)
   to `burned_source_digest` and the burn-required gate; add the burn-then-recode DoD-3
   variant.
2. **M2** — pin the burn composition explicitly: `redact_page`×N → `iw.write(cursor)` →
   `PdfFile::parse` → `rewrite_pdf`; name `iw.document()` as the forbidden shortcut.
3. **m1–m5** as listed (PNG encode step; burned-extension policy; generation mechanism
   ownership; from-hits zero-hit honesty; DisplayList construction into Phase-0).
4. **O1–O2** foldin's discretion.

`/foldin 0114` folds this file into spec/plan (fold review files only; do not implement here).