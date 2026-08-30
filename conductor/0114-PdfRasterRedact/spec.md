# 0114 — PDF raster + geometric redaction (`zpdf`)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export (0108–0109),
> matter-home overview (**0110**), first-pass queue (**0111** / **0117**),
> three-pane coding (**0112** / **0118**), DAT produce wizard (**0113** / **0119**),
> OPT (**0115** parked), or Process fold (**0116**). Do not vendor
> `C:\dev\dedupe-frontend`. Do not mint a BCC-default track.

- **Track ID:** 0114-PdfRasterRedact
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes `E-Discovery — ideal frontend` Image tab + produce Burn. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-30); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (density, not tokens, not pdf.js).
- **Status:** Completed
- **Depends on:** **0112 Completed** (PR **#115** / `81a3aad`; Image tab stub) · **0113 Completed** (PR **#117** / `f192b2d`; Burn step copy names this track) · redaction **0032** (`item_redactions` + `redacted_text_sha256`) · PDF extract **0034** (`extract-pdf` detect/caps; no raster) · produce **0040** (`resolve_native` copies original `native_sha256`) · QC **0041** · `matter-core` schema **v39** (this track bumps **v40**)
- **Spec authored:** 2026-08-30 (placeholder → Ready)
- **Series:** O (Review chrome) — fifth track
>
> **Closes / absorbs:** `D-0032-01` (PDF geometric burn + jpeg/png paint-burn) and `D-0034-02` (page raster preview). Closes `D-0034-04` as a duplicate of D-0032-01. Partial absorb of **D-0030-01** (geometric boxes; text path already 0032) and **D-0026-03** (Image tab; text/HTML already 0112). Does **not** close D-0034-05 (full Acrobat-class viewer), D-0034-03 (required pdfium bundle), D-0040-01 / D-0060-04 (0115), D-0032-02 (Office native redact), D-0117, D-0118, D-0119.
> **HITL:** owner launches the **release** EXE, opens a **synthetic** PDF item whose uncompressed content stream contains `SECRET_TOKEN_0114`, draws a box, Burns, Finalizes a DAT volume, and confirms the produced native does **not** contain the token. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-08-30):** PRs **#118, #117, #116, #115**. Disposition in §2.8. Three **0113** produce Bugbot items **minted 0119**. Window Bugbot stays **0118**. Queue Bugbot stays **0117**.
>
> **Review fold-in (2026-08-30):** `opencode-review.md` + `agy-review.md`. Disposition in §2.10 and `foldin-note.md`. Locks: fingerprint includes 0032 text state; burn is `redact_page` → incremental `write` → `PdfFile::parse` → `rewrite_pdf` (`iw.document()` forbidden); host CropBox/`/Rotate` map; `ITEM_COLUMNS` **append**; `matter-qc` `burned_native_missing` Error; this track mints raster `generation`.
>
> **Stack lock (inherit 0110–0113):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / **blocker** / **draft redact overlay** only. No daemon. **No `process-runner` on chrome.** No 0117/0118/0119 ID reuse for raster.

---

## 1. Objective

Replace the **0112** Image-tab stub (`No raster yet (0114).`) with a **Rust-side page raster** plus **draft geometric boxes**, and replace the **0113** Burn copy (`Geometric PDF burn is 0114`) with a **true content-stream burn** so a produced PDF native cannot yield redacted text by copy, extract, or file-byte search.

This advances **production defensibility**. Today 0032 only redacts **TEXT/** (`redacted_text_sha256`); `matter-produce::resolve_native` still copies original `native_sha256`. A PDF with privileged passages therefore ships the secret in **NATIVES/**. Overlay-only “black boxes” also fail. This track burns the **content stream**, then **rewrites** the PDF so prior revisions are not recoverable.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0112 Completed:** Image tab exists as an honest stub; `r` focuses it. **0113 Completed:** DAT produce is live; Burn step still names this track. Unique-export Series S is closed. The remaining counsel gap after DAT is **visible pages + honest native burn**.

### 2.2 Live APIs (plan-time 2026-08-30, HEAD `6ca24be`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 39`. This track **bumps to 40**. |
| `item_redactions` | UTF-8 **char** ranges on display text (v13). `reason` ∈ privilege/pii/confidential/other. Status `active`/`stale`. **Do not** overload this table with PDF user-space rects. |
| `Item` | Has `redaction_count` / `redacted_text_sha256` / `redacted_text_at` / `redacted_source_digest` (`matter.rs` ~256–262). `ITEM_COLUMNS` + `map_item_row` use **positional** `row.get` (`redaction_count` = 66 … last current column `teams_extract_error`). **Append** v40 columns at the **end** of `ITEM_COLUMNS`. Do **not** insert after `redacted_source_digest` (that would shift office/pdf/… indices). |
| `Matter::create_redaction` / `regenerate_redacted_text` | Text path only. Original `text_sha256` / native CAS **never** rewritten. |
| `extract-pdf` | Text extract. `MAX_NATIVE_INPUT_BYTES = 100 MiB`, `MAX_PAGES = 500`. Encrypted → `pdf_encrypted`. `looks_like_pdf` / `is_pdf_eligible_meta`. `#![forbid(unsafe_code)]`. **Do not** add zpdf here. |
| `matter-core::pdf` | `apply_pdf_text` bookkeeping. Comment: raster **not** in P0. |
| `matter-produce::resolve_native` | Copies `native_sha256` CAS to `NATIVES/` even when `redaction_count > 0`. Text path fail-closes `redacted_text_missing`. This track adds burned-native fail-closed. |
| Chrome Image tab | `review_window.rs` copy `"No raster yet (0114)."` Overlay `r — Image tab (0114 stub)`. |
| Chrome produce Burn | `produce.rs` copy `"Only CAS redacted text is packaged. Geometric PDF burn is 0114. Highlights never burn."` |
| Chrome workers | `join_worker` + `std::thread::spawn`. **No** `process-runner` dep (`dedupe-chrome/Cargo.toml`). |
| CSP | `img-src: 'self' data:` already (`tauri.conf.json`). PNG data URLs OK. **Do not** add `blob:` / `unsafe-inline` scripts. **Do not** put PDF bytes in the WebView. |
| Highlights | `item_highlights` (0030). Yellow paint. **Never** burn. |
| Fixtures | `fixtures/pdf/{minimal,empty,low_text,corrupt}.pdf`. Tests may generate additional synthetic PDFs in `tempfile`. |
| CI | `chrome-ui`: wasm32 + `trunk` **0.21.14** + `cargo test -p dedupe-chrome`. Keep it. `ui/` stays workspace-excluded. |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only; re-verified 2026-08-30)

Hermes Image tab: raster + redact tool (`r`). Produce Burn step: confirm burns before Finalize. Mock still has no `/review/:docId` raster. Do not vendor coral. Do not ship pdf.js.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `zpdf` | **0.13.0** (crates.io 2026-08-25, MIT) | Facade + **CPU** renderer. Feature **`cpu-render`** (default; pulls `zpdf-render-cpu`). **Do not** enable `gpu-render`. Re-verify feature names at execute. |
| `pdfium-render` | n/a this DoD | **Optional residual** (§3.10). Do **not** take crates.io `pdfium` (**GPL-3.0**). |
| `tauri` | **2.x** stable | Reject 3.x / pre-release. |
| `leptos` / `leptos_router` | **0.8** CSR | Reject 0.9-beta. |
| Rust | **stable** | No nightly. |

Online (2026-08-30, zpdf 0.13.0 source): `IncrementalWriter::redact_page(page_index, rects: &[Rect], options: &RedactOptions)` — PDF user space **y-up**; drops intersecting operators and overlapping annotations; Form XObjects are **not** descended (whole XObject dropped if placement intersects). `RedactOptions.fill` defaults to black. `IncrementalWriter::write` appends an incremental update; `iw.document()` is the **base** graph (pending edits **not** visible). `rewrite_pdf(&PdfFile, &RewriteOptions) -> Vec<u8>` GCs orphans. `ContentInterpreter::new(page.effective_box()).with_page_rotation(page.rotate)` → `DisplayList` (CropBox ∩ MediaBox + `/Rotate`). `CpuRenderer::render_display_list(&list, scale) -> RenderedPage { width, height, data: RGBA }` — **not** PNG; encode with `image` (or equivalent) to PNG bytes. `RenderedPage::save_png(path)` writes a file and **consumes** the page — do not use that as the IPC path. `search_spans` never matches across line boundaries (one rect per matched line). Encrypted → `IncrementalWriter::new` errors (no password UI).

**Produce-burn lock (normative composition):**

```text
IncrementalWriter::new(original)
  → redact_page × N
  → iw.write(&mut Cursor<Vec<u8>>)     // incremental bytes; not the produce artifact
  → PdfFile::parse(written, limits)
  → rewrite_pdf(&parsed, &RewriteOptions)  // GC; this Vec<u8> is the CAS native
  → put_bytes
```

`rewrite_pdf(iw.document(), …)` **compiles and is wrong** (rewrites the unredacted base). `IncrementalWriter::write` **alone** is also forbidden (file tail keeps the original stream). If `rewrite_pdf` is gone at execute, **stop**.

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3983 pinned at fold-in).
- Recall: 0034 shipped text-only, **no** pure-Rust raster; 0032 text `[REDACTED]` token; 0060 no image/OPT; 0112 Image stub until this track; 0113 DAT-only, Burn names 0114, Highlights never burn. Stale “frontend uses 0106+” superseded by Series O **0110–0113 Completed**.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` **0 pending / 0 unaudited drift** before this tx. Doctor: phantom-promote, sig-pin, completion-unreachable, impact-stale — none block planning.
- Ledger tx for this planning pass: `3f43e5c4-7fb0-4adb-aa47-b1524d870a02`.
- Fold-in tx: `57006a8f-7bf4-4c00-8f3b-0366c89fc34f`.
- `scan --impact` after spec write (docs/conductor + 0119 mint expected **LOW**).

### 2.6 How this advances the north star

Not UI polish: produced PDF natives must **not** contain redacted content in the file bytes or in a text extract. A raster that is only a screenshot, or a burn that is only an overlay annotation, **fails the track**. Unique-pst CLI is unchanged.

### 2.8 Last-PR Cursor comments (mandatory)

Last 4 merged: **#118** (docs 0113 Completed), **#117** (0113 product), **#116** (docs 0112 Completed), **#115** (0112 product).

| PR | Surface | Disposition |
|---|---|---|
| **#118** | docs registry | none |
| **#116** | docs registry | none |
| **#115** | `review_window.rs` / `path_id.rs` | three Bugbot items already **0118**. Do not steal. **New** raster commands in this track **must** ignore stale replies (`item_id` + generation) so 0114 does not ship the same class of bug on a new invoke. |
| **#117** | `produce.rs` (host + UI) | three **valid** Bugbot items, **not this track** — **minted 0119**: (1) High — Finalize stays armed after success / privilege-log failure still retries same Bates; (2) Medium — empty `filter_ids` `Some([])` skips `IN` and exports the whole log (`privilege.rs` 927–936 and 987–994); (3) Medium — QC/overrides/step survive matter route change. Live-verified 2026-08-30. |

### 2.9 Industry / license (plan-time)

- True redaction = content-stream excision + no recoverable prior revision, not a black annotation.
- FRCP 5.2 / privilege: same reason vocab as 0032.
- **deny.toml** allow-list: MIT / Apache-2.0 / BSD-3-Clause. zpdf MIT fits. Strong copyleft is denied — no MuPDF, no Poppler bundled, no GPL `pdfium` crate.
- PDFium library itself is BSD-3 + Apache-2.0; the packaging cost is the **sidecar DLL**, not the license. Optional (§3.10).

### 2.10 Review fold-in (2026-08-30)

Sources: `opencode-review.md`, `agy-review.md`. Harness files **not** edited. Live-verified against `ITEM_COLUMNS`/`map_item_row` positional `row.get`, `matter-qc` `RULE_REDACTED_TEXT_MISSING`, `extension_from_item`, Image-tab Effects with **no** generation, zpdf 0.13.0 README (`effective_box` + `with_page_rotation`).

| Id | Disposition |
|---|---|
| **opencode-M1** | **Fold** — fingerprint + burn-required include 0032 text state; DoD-3 post-burn recode |
| **opencode-M2** / **agy-M2** | **Fold** — named compose; `iw.document()` forbidden. agy-M2 was intent; composition was the hole |
| **agy-M1** | **Fold** — CropBox ∩ MediaBox + `/Rotate`; host maps raster pixels → user space |
| **agy-M3** | **Fold** — `matter-qc` Error `burned_native_missing` (and unmapped-text) like `redacted_text_missing`; not chrome-only |
| **opencode-m1** | **Fold** — RGBA → PNG encode via `image` |
| **opencode-m2** | **Fold** — burned codec matches `FILE_EXT` (jpeg stays jpeg) |
| **opencode-m3** / **agy-m2** | **Fold** — this track **mints** Image-tab `generation`; 0118 still owns document/body |
| **opencode-m4** | **Fold** — `search_spans` line-bounded; zero-hit → unmapped blocker |
| **opencode-m5** | **Fold** — DisplayList construction in Phase 0 |
| **agy-m1** | **Fold** — **append** v40 columns at end of `ITEM_COLUMNS` |
| **agy-O1** | **Fold** — 1 pt dilation on `source=hit` rects |
| **opencode-O1** | **Fold** — `cargo test -p matter-core` (no `--lib` only) |
| **opencode-O2** | **Fold** — prefer zpdf `EncryptionConfig` fixture; stand-in only if API gone |

---

## 3. In scope

### 3.1 Placement

| Component | Location |
|---|---|
| Schema v40 + geom API | `matter-core` (`geom_redaction.rs` or `redaction.rs` sibling; **new table**) |
| Raster + PDF burn engine | **New** workspace crate `pdf-raster` (name as you like; not `extract-pdf`) |
| Produce native resolve | `matter-produce::resolve_native` |
| Chrome Image tab + Burn | `dedupe-chrome` host + `ui/` Image pane + produce step 4 |
| Fixtures / tests | `tempfile` synthetic PDFs; optional `fixtures/pdf/` additions if tiny and synthetic |
| Desk raster UI | **Out.** Engine APIs may be called later; do not fold egui Process (**0116**). |

`pdf-raster` is blocking/CPU. Callers run it on `join_worker` (chrome) or a dedicated thread. Never on the WebView/Tokio UI thread. `catch_unwind` at document boundary.

`extract-pdf` stays text-only and `forbid(unsafe_code)`.

### 3.2 Schema v40 (normative)

#### 3.2.1 `item_geom_redactions`

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | |
| `item_id` / `matter_id` | TEXT NOT NULL | |
| `page_index` | INTEGER NOT NULL | **0-based** |
| `x` `y` `w` `h` | REAL NOT NULL | PDF user space, **y-up**, origin bottom-left. `w>0`, `h>0`. Map to `zpdf::Rect { x0: x, y0: y, x1: x+w, y1: y+h }`. |
| `reason` | TEXT NOT NULL | Same vocab as 0032: `privilege` \| `pii` \| `confidential` \| `other` |
| `label` | TEXT NULL | Stamp metadata only |
| `status` | TEXT NOT NULL | `active` \| `stale` |
| `source` | TEXT NOT NULL | `draw` \| `hit` \| `full_page` |
| `created_at` / `updated_at` | TEXT | RFC3339 |
| `created_by` | TEXT | |

Indexes: `(item_id, page_index)`; `(matter_id, status)`.

Hard delete of a region is OK if audit retains the rect snapshot (same pattern as text redactions).

**Do not** store CSS pixels. Overlay converts using the raster’s page width/height in points.

#### 3.2.2 Item bookkeeping

| Column | Meaning |
|---|---|
| `geom_redaction_count` | INTEGER NOT NULL DEFAULT 0 (active rows) |
| `burned_native_sha256` | TEXT NULL — CAS of last successful burned native |
| `burned_native_at` | TEXT NULL |
| `burned_source_digest` | TEXT NULL — fingerprint of `native_sha256` + canonical active **geom** list + **0032 text state** (`redacted_text_sha256` or a count+max(`updated_at`) of active `item_redactions`) + engine pin (`zpdf-0.13.x`) |
| `raster_engine` | TEXT NULL — `zpdf` (required path). `pdfium` only if §3.10 ships |

**`ITEM_COLUMNS`:** append these five columns after the last current field (`teams_extract_error`). Leave indices `0..N` intact.

Native SHA-256 change **or** 0032 text-redaction change **or** geom-list change → fingerprint mismatch → burn required. NULL `burned_*` on native change; mark geom `stale` when native changes.

Original `native_sha256` CAS is **never** overwritten.

### 3.3 Raster (Image tab)

Eligible natives:

| Kind | Engine |
|---|---|
| `application/pdf` (detect via `extract_pdf::detect_pdf` **or** copied magic — do not depend on `dedupe-desk`) | zpdf CPU `DisplayList` → RGBA → **PNG bytes** (`image` encoder) |
| `image/jpeg` / `image/png` | `image` crate decode → PNG for the tab (no zpdf) |
| Other (EML, OOXML, TIFF, …) | Honest empty: “Not a page image (TIFF/OPT is **0115**).” |

Rules:

- Visible page first. Thumbs / other pages may Rayon **inside** the worker; cap concurrency.
- DPI: **150** review, **72** thumb. Long-side pixel cap **4096**.
- Caps: 100 MiB native, 500 pages (align `extract-pdf`). Beyond cap: honest truncated banner, not a hang.
- Encrypted PDF → `pdf_encrypted` empty state. No password UI (**D-0034-06** never).
- Corrupt / panic → `catch_unwind`, item error copy, other items continue.
- Cache: derived CAS key `raster-v1|{native_sha256}|p{page}|dpi{n}|zpdf-{ver}` optional. In-process LRU (e.g. 32 pages) required so tab switches are not a full re-parse every time.
- DisplayList: `ContentInterpreter::new(page.effective_box()).with_page_rotation(page.rotate)` then `CpuRenderer::render_display_list`. Encode `RenderedPage.data` (RGBA) to PNG **bytes** in the host — do not call `save_png` for IPC.
- Host returns PNG bytes (or base64) + page count + **MediaBox, CropBox (or `effective_box`), `/Rotate`**, raster width/height. UI paints `<img src="data:image/png;base64,…">` plus overlay. **Never** `innerHTML` with PDF. **Never** pdf.js.
- **Coordinate map (host-owned):** DB stores PDF user space (y-up). Overlay drags are raster pixels (CSS y-down). `review_geom_upsert` accepts pixel box + page_index; host applies inverse of CropBox origin + `/Rotate` + y-flip before persist. Painting reads stored user-space through the same transform. A `/Rotate 90` (or non-zero CropBox origin) fixture is DoD — a box on the visible token must burn **that** token, not a neighbor.

Page nav when Image tab focused: `,` / `.` or PageUp / PageDown. **Do not steal** 0112 `[` `]` (document neighbors).

`r` still focuses Image. Overlay `?` updates: drop “(0114 stub)”.

### 3.4 Draft boxes (viewer)

Boxes are **drafts** until Burn. Overlay: translucent hatch (privilege red `#9B2C2C` at low alpha, not coral). Distinct from 0032 text blackout and from 0030 yellow highlights.

- Draw: drag on the raster → `source=draw`.
- Full page: one MediaBox rect, `source=full_page` (absorbs a slice of **D-0032-07**; **inverse** redact stays residual).
- Delete: selected box / Esc cancel draw.
- List in the coding pane or a thin Image-tab strip: reason + page + delete.

Highlights **never** become geom rows.

### 3.5 Redact-from-hits

When the PDF has a text layer, **Redact from hits** (button on Image tab; optional when Text tab has an active 0032 region):

1. Take `item_redactions.exact_quote` (active) and/or the in-doc find string.
2. `zpdf::search_spans` per page. Matches **never cross line boundaries** (one rect per matched line). A multi-line `exact_quote` that finds **zero** hits is an honest miss → if `redaction_count > 0` and geom stays empty, the unmapped blocker fires. Do not silently skip.
3. Insert geom rows `source=hit` for each hit rect, **dilated by 1 PDF point** on each side (ascender/descender bleed). This is the **D-0032-11** slice: all line-accurate instances of that quote on the page.
4. If a PDF has `redaction_count > 0` and **zero** geom rows after mapping, **blocker** `text_redact_unmapped_on_pdf` (engine QC + chrome). Counsel must draw boxes or withhold. **Do not** produce the original native.

### 3.6 Burn (produce + Image)

Burn writes a **new** CAS native; it does not mutate source PST or original CAS.

**PDF** — composition is §2.4 (write-to-cursor → parse → `rewrite_pdf`). `iw.document()` is forbidden. `RedactOptions` fill **black**.

**JPEG/PNG:** paint filled black rects in pixel space and re-encode with the **same codec as the produced extension**. JPEG source → JPEG bytes + `FILE_EXT=jpg`. PNG source → PNG + `png`. **Do not** write PNG bytes under `.jpg` (`extension_from_item` would otherwise keep `jpg`). If JPEG encode fails, fail-closed — do not silently swap codecs. Not TIFF G4 (**0115**).

**When burn is required** (fail-closed `burned_native_missing` / stale fingerprint):

- `geom_redaction_count > 0`, or
- PDF native **and** `redaction_count > 0`, or
- `burned_source_digest` missing/mismatch (geom **or** 0032 text state changed after the last burn).

`resolve_native` copies `burned_native_sha256` when required; **never** falls back to original `native_sha256`. Produced `FILE_EXT` / mime follow the **burned payload**, not a mismatched source extension.

Chrome produce step 4:

- Counts: need-burn / burned-fresh / unmapped-text.
- **Burn selected set** button → `join_worker` (same Option C as 0113: no process-runner).
- Copy: “Highlights never burn. Draft overlays are not the produced native.”
- Drop the sentence “Geometric PDF burn is 0114.”

Burn of an empty-geom PDF is a no-op success (unless text-redact-unmapped).

### 3.7 Chrome commands

All on `join_worker`. Encrypted matter → `encrypted`. Actor `"chrome"`. Autogenerate `allow-*` permissions. No `fs:default`.

Suggested names (lock the contract, not the ident):

| Command | Role |
|---|---|
| `review_raster_page` | PNG + page_count + MediaBox/CropBox/`Rotate` + raster size. **Ignore stale.** |
| `review_geom_list` | Active/stale boxes for item (user space). |
| `review_geom_upsert` / `review_geom_delete` | Pixel box in; host converts; audit. |
| `review_geom_from_hits` | §3.5 |
| `review_burn_native` | Burn one item. |
| `produce_burn_set` | Burn every required item in the current produce id list (pre-flight). |

**Stale-reply guard (this track mints it):** Image tab holds a Leptos `raster_generation: RwSignal<u64>` (or equivalent) bumped on `doc_id` change **and** on page change. Pass `item_id` + generation on raster/geom invokes; UI discards mismatches. **No** generation exists on the 0112 document/body Effects today (`review_window.rs` ~163–239) — that remains **0118**. Do not look for a shared counter that is not there.

Do **not** extend `ReviewListRow`.

### 3.8 Produce / QC extras

`resolve_native` fail-closed remains the last gate. **Also** add engine QC rules (CLI/Desk preflight, not chrome-only extras), same pattern as live `RULE_REDACTED_TEXT_MISSING` (`matter-qc/src/rules.rs`):

| id | default | when |
|---|---|---|
| `burned_native_missing` | **Error** on default pack | burn required (geom count > 0 **or** PDF ∧ `redaction_count > 0`) **and** (`burned_native_sha256` missing **or** fingerprint mismatch) |
| `text_redact_unmapped_on_pdf` | **Error** on default pack | PDF + `redaction_count > 0` + zero active geom rows |
| `pdf_raster_failed` | chrome **warning** extra only | Image tab failed; produce still blocked if burn required |

Do not auto-re-QC. Membership drift still 0113 stale gate. Adding rules to the default pack makes prior QC fingerprints stale — correct.

### 3.9 Keyboard / a11y / tokens

Inherit 0110–0112. Image-focused: `,` `.` pages; draw tool mouse-primary; `r` focuses Image (now live). `[` `]` remain document neighbors. Skip-to `#document` still lands on the raster. Focus-visible on boxes. No `#ec3013`.

### 3.10 pdfium fallback (optional, not DoD)

Placeholder promised per-doc pdfium when zpdf is ugly. Packaging a `pdfium.dll` next to the single EXE is **not** required to close D-0034-02.

Lock:

- zpdf CPU is the **required** path.
- Cargo feature `pdfium-fallback` **off** by default. If enabled, load `pdfium.dll` from the EXE directory via `pdfium-render` (MIT/Apache) behind the crate’s process mutex (Pdfium is not thread-safe). Missing DLL → stay on zpdf, honest banner.
- **Do not** vendor the DLL in git. **Do not** take GPL `pdfium` crate.
- Residual **D-0114-pdfium-sidecar** if the feature does not ship.

Ugly zpdf raster on a hostile PDF is acceptable if the Image tab still shows *a* page or an honest error. Do not block DoD on pixel-perfect Adobe fidelity.

### 3.11 Hygiene

- Production: no `unwrap` / `expect`. `main` still `Result`.
- Never mutate source PSTs / Purview. Never commit client PDFs, `output/`, `evidence/`.
- Tests: `tempfile` + `insert_family` → parent → children; `ensure_default_review_set` then `in_review`; `put_bytes` for a tiny uncompressed PDF. No client evidence.
- New workspace member + `LicenseRef-Proprietary` like siblings. `cargo deny` must stay green (zpdf MIT).
- `ui/` stays excluded (no zpdf on wasm).

---

## 4. Out of scope (do NOT do here)

- **0119** produce-wizard Bugbot (Finalize re-arm, empty privilege-log filter, QC state across matters).
- **0117** queue header/spacer / vacant lie / arrow scroll.
- **0118** existing `review_document` / `review_document_body` stale fetch (new raster commands still need their own guard).
- **0115** TIFF G4 / OPT / `IMAGES/` / page-level Bates.
- **0116** process-runner on chrome / egui Process fold / multi-GB cancel.
- Desk Image-tab raster UI.
- Native DOCX/XLSX redact (**D-0032-02**).
- Inverse redact, stamp-in-token, metadata-field redact, AI suggested ranges.
- pdf.js / MuPDF / Poppler / GPL pdfium crate / wgpu GPU renderer.
- Password recovery (**D-0034-06** never).
- OCR of burned natives (**D-0036-08**).
- Full multi-page Acrobat chrome (**D-0034-05** residual): this track is visible page + prev/next, not thumb-strip + outline + form fill.
- Schema beyond v40, unique-pst flags, BCC-default.
- Vendoring the mock. Axum daemon. Leptos SSR. tauri 3.x. leptos 0.9.
- Extending `ReviewListRow`. Chrome `connection()` SQL.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0112 Image tab + 0113 produce wizard on main (HEAD `6ca24be`).
- **P2:** zpdf **0.13.x** still MIT on crates.io; `cpu-render` default; `redact_page` + `rewrite_pdf(&PdfFile)` exist. If rewrite is gone at execute, **stop** and do not ship incremental-only burn.
- *Verified to date:* schema 39; produce copies original native; chrome Burn/Image copy names 0114; CSP `img-src data:`; extract-pdf caps; PR #117 three items live.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Incremental save leaves secret in file tail | Named compose (§2.4). Byte + extract oracle. |
| `rewrite_pdf(iw.document())` ships original | Forbidden shortcut. Tests must fail if token remains. |
| Post-burn 0032 text redaction still in native | Fingerprint includes text state → stale → re-burn. |
| `/Rotate` / CropBox mis-burn | Host inverse map; Rotate-90 fixture in DoD. |
| Form XObject text not descended | zpdf drops intersecting XObject. Residual **D-0114-xform-text** if under-redact cannot be closed. Prefer fail-closed. |
| `ITEM_COLUMNS` / `row.get` shift | **Append** at end. Do not insert after redaction columns. |
| PNG-in-`.jpg` DAT | Codec matches extension. |
| Hostile PDF hang/OOM | Caps + `ParseLimits` + CpuRenderer budgets + `catch_unwind`. |
| pdfium DLL / GPL crate | DoD does not require pdfium. Feature off. Deny GPL. |
| Ugly raster vs Adobe | Honest preview; burn is content-stream, not the PNG. |
| Stale raster paints wrong doc | Image-tab `generation` minted here; 0118 owns document/body. |
| Long burn freezes IPC | DoD fixture small; residual stays **D-0113-long-job** / **0116**. |
| License drift | `cargo deny`; pin recorded in CHANGELOG. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Image tab rasters:** PDF fixture (`%PDF` + uncompressed stream containing `SECRET_TOKEN_0114`) in a tempfile matter → Image tab returns a PNG (`content-type` image, not empty stub copy). Page 2 of a 2-page synthetic PDF is reachable with `,`/`.` (or PageDown). JPEG/PNG native shows decoded pixels. EML shows “Not a page image”. Encrypted PDF via zpdf `EncryptionConfig` (RC4 or AES; stand-in only if that API is gone at execute) shows `pdf_encrypted` / honest error, not a blank hang. A `/Rotate 90` (or CropBox origin ≠ 0) fixture: a box drawn on the **visible** token burns that token, not a neighbor. Copy `"No raster yet (0114)."` is **gone**. `dedupe-desk` still builds. No `process-runner` on `dedupe-chrome`. No zpdf in `ui/`.
- [ ] **DoD-2 — Draft ≠ burned:** Creating a geom box does **not** change `native_sha256`. Overlay is present in the Image tab JSON/list. A produce **without** Burn fail-closes (`burned_native_missing`) when `geom_redaction_count > 0`. Highlights on the same item do **not** appear as geom rows and do **not** change the burned file.
- [ ] **DoD-3 — True burn:** After Burn, `burned_native_sha256` is a **different** digest from `native_sha256`. Burned file bytes do **not** contain `SECRET_TOKEN_0114` (UTF-8 and UTF-16LE). zpdf (or extract-pdf) text of the burned file does **not** contain the token. Original CAS blob **still does**. `resolve_native` for that item writes the burned digest’s bytes under `NATIVES/`, not the original. Incremental-only output (original stream still in the file) **fails this DoD**. **Variant:** burn, then `create_redaction` of a *second* token with no new geom → fingerprint stale → produce **refuses** until re-burn (does not ship the new words). JPEG burn writes JPEG bytes with `FILE_EXT=jpg` (not PNG magic).
- [ ] **DoD-4 — Text-redact-on-PDF honesty:** PDF with an active 0032 text redaction of the token and **no** geom rows → QC Error `text_redact_unmapped_on_pdf` (or from-hits creates geom that then burns). Produce of original native is refused. From-hits on a text-layer fixture creates `source=hit` boxes that cover the token. Multi-line quote with zero `search_spans` hits is unmapped, not a silent skip.
- [ ] **DoD-5 — Schema + CI:** `SCHEMA_VERSION == 40`. v40 columns are **appended** on `ITEM_COLUMNS` (office/pdf `row.get` indices unchanged). Geom create/list/delete + fingerprint mismatch + `matter-qc` `burned_native_missing` covered by tests (`tempfile`). New commands have `allow-*`. Encrypted root → `encrypted`. `cargo test -p pdf-raster` (or crate name) + `cargo test -p dedupe-chrome` + `cargo test -p matter-produce` + `cargo test -p matter-qc` + `cargo test -p matter-core` + `cargo check -p dedupe-desk`. Workspace fmt/clippy/test + `chrome-ui` trunk stay green. `cargo deny` accepts zpdf MIT. No production `unwrap`/`expect`. CSP still no PDF in `img-src` except `data:` PNG.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0032-01` and `D-0034-02` closed (image TIFF half of D-0032-01 declined → 0115); `D-0034-04` closed as duplicate; ledger committed (`FEATURE`). **0115** parked. **0116** / **0117** / **0118** / **0119** stay Proposed.

**Owner HITL (not CI):** release EXE, synthetic PDF, draw box, Burn, Finalize DAT, open produced native — token gone. INC* waived.

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p pdf-raster
cargo test -p dedupe-chrome
cargo test -p matter-core
cargo test -p matter-qc
cargo test -p matter-produce
cargo check -p dedupe-desk
# rustup target add wasm32-unknown-unknown
# trunk build --config crates/dedupe-chrome/ui/Trunk.toml --release
```

---

## 9. Deferred (absorb / decline)

| Row | Disposition |
|---|---|
| **D-0032-01** geometric PDF/image burn | **Absorb / close** for PDF content-stream + jpeg/png paint-burn. TIFF G4 remains **0115**. |
| **D-0034-02** raster preview | **Absorb / close** (visible page + prev/next). |
| **D-0034-04** geometric burn-in | **Close** as duplicate of D-0032-01. |
| **D-0030-01** Image/PDF box markups | **Partial** — geometric boxes here. Text path already 0032. Re-owner any leftover to closed-in-0114. |
| **D-0026-03** HTML/image body | **Partial** — Image raster. Text/HTML already 0112. Do not close the row if HTML residuals remain; Image half done. |
| **D-0032-07** inverse / full-page | **Partial** — `source=full_page` in. Inverse **decline** (residual). |
| **D-0032-11** redact-all-instances | **Partial** — from-hits all spans of the quote. Manual multi-string still residual. |
| **D-0032-10** MuPDF / `redactor` | **Decline.** License. zpdf instead. |
| **D-0034-03** required pdfium/MuPDF bundle | **Decline** as required. Optional sidecar residual **D-0114-pdfium-sidecar**. |
| **D-0034-05** full interactive PDF viewer | **Decline** (page nav only). |
| **D-0034-06** password bypass | **Never.** |
| **D-0032-02** Office native redact | **Decline.** |
| **D-0032-05** / **D-0032-06** / **D-0032-09** / **D-0032-12** / **D-0032-13** | **Decline** (AI, metadata, fixed-width token, load-file fields, stamp-in-token). |
| **D-0036-08** OCR after burn | **Decline.** Do not OCR burned natives this track. |
| **D-0040-01** / **D-0060-04** TIFF/OPT | Remain **0115** parked. |
| **D-0117** / **D-0118** | Remain. |
| **D-0119-produce-checklist-residuals** | **Minted** this pass from PR #117. Remain Proposed. |
| **D-0113-long-job** | Remain / **0116**. |
| **D-0110-deny-unic** | Remain residual / upstream. |
| **D-0116-process-fold** | Remain. |
| **D-0062-codesign** | Release ops. **Decline.** |
| **D-0108-keepset-crc-retaint** | Unique-export. **Decline.** |
| Last-PR #117 three produce items | **Minted 0119.** |
| Last-PR #115 three window items | **0118.** |
| Last-PR #113 queue items | **0117.** |
| Mock pdf.js / coral | **Decline.** |
| opencode-M1 text fingerprint | **Folded** — §3.2.2 / DoD-3 variant. |
| opencode-M2 / agy-M2 compose | **Folded** — §2.4 pipeline. |
| agy-M1 CropBox/Rotate | **Folded** — §3.3 host map + DoD-1 fixture. |
| agy-M3 matter-qc rule | **Folded** — §3.8 Error on default pack. |
| opencode-m1..m5 / agy-m1 m2 / O1–O2 / agy-O1 | **Folded** as §2.10. |

---

## Series O index (do not reorder)

| ID | Item | After this plan |
|---|---|---|
| **0110** | Matter chrome + one overview command | **Completed** (PR **#111** / `5a76f0b`) |
| **0111** | Virtualized first-pass queue | **Completed** (PR **#113** / `3c4ca65`) |
| **0112** | Three-pane review window | **Completed** (PR **#115** / `81a3aad`) |
| **0113** | Produce checklist; DAT only | **Completed** (PR **#117** / `f192b2d`) |
| **0114** | zpdf raster + geometric redact | **Completed** (PR **#119** / `5ed53bf`) |
| **0115** | TIFF G4 + OPT | **Parked** |
| **0116** | Fold egui Process | Proposed |
| **0117** | Queue virtualization residuals (PR #113) | Proposed |
| **0118** | Review-window async residuals (PR #115) | Proposed |
| **0119** | Produce-checklist residuals (PR #117) | **Proposed — placeholder** |
