# 0115 — TIFF G4 + Opticon OPT factory

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export (0108–0109),
> matter-home overview (**0110**), first-pass queue (**0111** / **0117**),
> three-pane coding (**0112** / **0118**), DAT-only default (**0113** / **0119**),
> zpdf geometric burn (**0114** / **0120**), or Process fold (**0116**).
> Do not vendor `C:\dev\dedupe-frontend`. Do not mint a BCC-default track.

- **Track ID:** 0115-ImageOptFactory
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes Image / produce Format (TIFF G4 + OPT). `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-30); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (density, not tokens, not coral, not fake `IMAGES/`).
- **Status:** Completed
- **Depends on:** **0114 Completed** (PR **#119** / `5ed53bf`; schema **v40**; `pdf-raster`; burned native) · **0113 Completed** (PR **#117** / `f192b2d`; DAT wizard; page-level Bates copy names this track) · produce **0040** · QC **0041** · profiles **0060** · `matter-core` schema **v40** (this track bumps **v41**)
- **Spec authored:** 2026-08-30 (parked placeholder → Ready)
- **Series:** O (Review chrome) — sixth track (un-parked)
>
> **Closes / absorbs:** `D-0040-01` (TIFF G4 + Opticon OPT image factory) and `D-0060-04` (image + OPT production profile; LFP stays residual). Closes the TIFF-G4 half of **D-0032-01** / **D-0030-01**. Does **not** close D-0042-06 (opposing OPT ingest), D-0040-10 slipsheets, D-0032-02 Office native redact, D-0116, D-0117, D-0118, D-0119, D-0120.
> **HITL:** owner launches the **release** EXE, picks the image profile, Finalizes a 2-page synthetic PDF (token already burned in 0114 path) plus one xlsx native-only sibling, opens `IMAGES/` + `IMAGE.opt` + `DATA/load.dat`. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-08-30):** PRs **#120, #119, #118, #117**. Disposition in §2.8. Three **0114** UI Bugbot items **minted 0120**. Produce wizard Bugbot stays **0119**. Window async stays **0118**. Queue stays **0117**.
>
> **Review fold-in (2026-08-30):** `opencode-review.md` + `agy-review.md`. Disposition in §2.10 and `foldin-note.md`. Locks: **explicit TIFF IFD** is the produce artifact (`fax::tiff::wrap` forbidden — 200 dpi + no BitsPerSample); `image_folder_cap ≥ MAX_PAGES` + fail-closed overflow; resume `advance_next_seq_for_control` uses **end_bates**; OPT/span checks read persisted rows; inbound TIFF decode-and-cap (not `DPI_REVIEW`); OPT volume token strips comma/space.
>
> **Stack lock (inherit 0110–0114):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / **blocker** / draft redact overlay only. No daemon. **No `process-runner` on chrome.** Default produce remains DAT-only.

---

## 1. Objective

Ship an **opt-in image production** next to the live DAT volume: **single-page TIFF CCITT Group 4** under `IMAGES/` plus a **seven-field Opticon `IMAGE.opt`**, with **page-level Bates** (`BEGBATES` ≠ `ENDBATES` when a document has more than one page). Default profile **`us_concordance_native_text_v1` stays DAT-only** (no `IMAGES/`, no OPT, document-level Bates). Spreadsheets / EML / OOXML stay **native-only** (DAT row, no OPT lines).

This advances **production defensibility**. Counsel protocols that require a Concordance + Opticon image set currently have no honest path — 0113 explicitly refuses fake `IMAGES/`. Raster and burn already exist in **0114**; this track turns burned (or original, when burn is not required) page rasters into the load-file pair receiving platforms actually ingest.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0114 Completed** (PR **#119** / `5ed53bf`): Image tab rasters PDF/JPEG/PNG; geometric burn writes `burned_native_sha256`; EML empty copy names this track; chrome Format/Number copy still says “TIFF / OPT off — ships in 0115.” Unique-export Series S is closed. The remaining counsel gap after honest PDF burn is **page images + OPT + page-level Bates**.

Un-park signal: `/plan-track 115` (2026-08-30). ROADMAP market-feature bucket still applies — this is not the default produce.

### 2.2 Live APIs (plan-time 2026-08-30, HEAD `64ae7f2`; product `5ed53bf`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 40`. This track **bumps to 41**. |
| `ITEM_COLUMNS` | Ends `geom_redaction_count … raster_engine`. **Do not** append item columns unless a stored per-item image digest is truly required — prefer `production_items` / `production_image_pages` (this spec). Positional `row.get` remains sacred. |
| `production_items` | One row per document. `control_number` unique per set. Engine assigns **one** sequence per item (`run.rs` `cursor.next_seq += 1`). DAT `BEGBATES` = `ENDBATES` = `CONTROL_NUMBER`. |
| `VolumeLayout` | `DATA/` + `NATIVES/` + `TEXT/` only. No `IMAGES`. |
| `ProduceParams` | No image flag. Profile slug default `us_concordance_native_text_v1`. `bates_start` required ≥ 1. |
| `LayoutConfig` | `data` / `natives` / `text` only. |
| `PackagingConfig` | `include_csv_twin`, `export_eml_if_missing_native`, `expand_family`. **No** `include_images`. |
| `BatesConfig` | `prefix`, `pad_width`, `filename_mode`. **No** page vs document mode. |
| `pdf-raster` | `raster_page(bytes, page_index, dpi, …)` → PNG. `DPI_REVIEW=150`, `DPI_THUMB=72`, `LONG_SIDE_CAP=4096`. `NativeKind` is Pdf/Jpeg/Png/**Other** — TIFF is Other. Caps 100 MiB / 500 pages. Burn compose is 0114; do not reopen. |
| `resolve_native` | Copies burned CAS when 0114 burn-required; else original. Image factory **must raster the same bytes** `resolve_native` would ship. |
| Chrome Format | Copy `"TIFF / PDF image / OPT off — ships in 0115. Do not create IMAGES/ or IMAGE.opt."` |
| Chrome Number | Page-level Bates button **disabled**; copy names this track. Family-together locked. |
| `matter-qc` | Default pack Errors include `burned_native_missing` + `text_redact_unmapped_on_pdf`. Image rules belong on a **new pack**, not as Errors on `qc_default_v1` (that would break DAT-only). |
| CI | `chrome-ui`: wasm32 + `trunk` **0.21.14** + `cargo test -p dedupe-chrome`. Keep it. `ui/` stays workspace-excluded. |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only; re-verified 2026-08-30)

Steal: Format-step density; page-level Bates as the **image** default; family-together locked.

**Do not copy / do not fake:** coral `#ec3013`; invented colour-sensitive page counts; slipsheets; LFP as default; `LOADFILES/VOL1.OPT` layout unless we also move DAT (keep DAT at `DATA/load.dat`); pdf.js.

### 2.4 Industry OPT / TIFF (plan-time 2026-08-30)

Court / vendor ESI examples (NY ED protocol; DISCO; OpenText Axcelerate; Relativity-oriented kCura OPT):

- **TIFF:** single-page, **CCITT Group 4**, **1-bit**, **300 dpi**, little-endian, `.TIF` / `.tiff`. Multi-page TIFF **not** supported as the produced artifact. Colour, when required, is typically JPEG — **out** (residual). Required IFD tags (normative, not wrap-defaults): Compression **4**, Photometric **0** (WhiteIsZero / MinIsWhite), BitsPerSample **1**, SamplesPerPixel **1**, FillOrder **1**, RowsPerStrip = height, **XResolution = YResolution = `image_dpi` (300)**, ResolutionUnit **2** (inches).
- **Folders:** ≤ **500–1000** files per `IMAGES\NNN\` folder; a document should not span folders.
- **OPT:** ASCII, **CRLF**, comma delimiter, **no text qualifier**, `.opt`. **Seven fields:** `ALIAS, VOLUME, RELATIVE_PATH, DOCUMENT_BREAK, FOLDER_BREAK, BOX_BREAK, PAGE_COUNT`. First page of a document: `Y` in DOCUMENT_BREAK and PAGE_COUNT = N; interior pages leave breaks and count empty.
- **DAT** stays document-level: `BEGBATES` = first page Bates, `ENDBATES` = last page Bates, `CONTROL_NUMBER` = `BEGBATES`.
- **LFP** (IPRO `IM`/`VOLM`) is the IPRO dialect. Most Relativity/DISCO/Everlaw ingest is **DAT+OPT**. LFP is **not** default (ROADMAP: “LFP default” is never-as-next-ID).

Example (normative):

```text
PROD000001,VOL001,IMAGES\001\PROD000001.TIF,Y,,,2
PROD000002,VOL001,IMAGES\001\PROD000002.TIF,,,,
PROD000003,VOL001,IMAGES\001\PROD000003.TIF,Y,,,1
```

Paths Windows-style, matching live DAT `NATIVES\…`.

### 2.5 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `fax` | **0.3.0** (crates.io 2026-07-13, **MIT**) | CCITT G4 **bitstream only**. `Encoder::new(VecWriter::new())` → per-row `encode_line(pels: impl Iterator<Item=Color>, width)` → `finish()` (two `EDFB_HALF` EOLs) → `VecWriter` bytes. **Do not** ship `fax::tiff::wrap` as the produce artifact (verified 2026-08-30: Compression=4 and Photometric=0 are correct, but **X/YResolution are hard-coded `Rational(200,1)`**, no tag 258 BitsPerSample, no 277 SamplesPerPixel, no 266 FillOrder). |
| `tiff` (image-rs) | **0.11.3** MIT | **Decode** (Fax4 decode exists). Encoder Compression enum is None/LZW/Deflate/PackBits only — **cannot write Fax4**. |
| `image` | **0.25** (already in `pdf-raster`) | Enable **`tiff`** feature for inbound TIFF **decode** only. Keep png/jpeg. Never treat that feature as G4 encode. |
| `zpdf` | **0.13.0** MIT | Unchanged; raster at `DPI_PRODUCE=300` (PDF only). |
| `tauri` / `leptos` | **2.x** / **0.8** CSR | Reject 3.x / 0.9-beta. |
| Rust | **stable** | No nightly. |

**G4 compose (normative — explicit IFD is primary):**

```text
bytes = burned_native if burn_required else original native
page  = pdf_raster::raster_page(bytes, i, DPI_PRODUCE=300, …)   // PNG/RGBA; TIFF inbound: decode 1:1 then LONG_SIDE_CAP
endorse Bates in lower-right on the raster BEFORE threshold (solid box, ≥0.25 in margin)
bilevel = 1-bit: ITU-R BT.601 luma (0.299R+0.587G+0.114B) < 160 → Black
g4    = Encoder::new(VecWriter::new()); for each row encode_line(Color iter, width); finish()
tif   = explicit little-endian IFD wrapping g4 (NOT fax::tiff::wrap)
      tags: 256/257 w/h, 258 BitsPerSample=1, 259 Compression=4, 262 Photometric=0,
            266 FillOrder=1, 277 SamplesPerPixel=1, 278 RowsPerStrip=height,
            282/283 X/YResolution = Rational(image_dpi,1), 296 ResolutionUnit=2
write IMAGES\<folder>\<BATES>.TIF
```

`fax::tiff::wrap` is a **verified-incompatible shortcut** (200 dpi tags). Do not call it for produce. If the explicit IFD path cannot write Compression=4 / 1-bit / 300 dpi tags, **stop** — PackBits/LZW/Deflate TIFF is not this track.

**Forbidden:** libtiff/CGo, ImageMagick, Ghostscript, MuPDF, Poppler, GPL `pdfium` crate, wgpu, pdf.js, multi-page TIFF output, `fax::tiff::wrap` as the volume artifact.

### 2.6 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; **3987** pinned at fold-in; planning-time 3985 is the same self-correcting drift — do not chase).
- Recall: 0113 DAT-only, page-level Bates is 0115; 0060 no image/OPT factory; 0114 TIFF/OPT parked here; 0114 Completed PR #119 / docs #120. Ready pin `ccbf7f15-84cc-47f7-ab92-de2761fe6290`.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` **0 pending / 0 unaudited drift** before this tx. Doctor: phantom-promote, sig-pin, completion-unreachable, impact-stale — none block planning.
- Ledger tx for the Ready pass: `805b9cf0-8155-49a1-b158-bf4b7e42665d` (committed).
- Fold-in tx: `6ccb6e4d-cb07-42b1-be44-e6a5ffe1f39c`.
- `scan --impact` after spec write: **LOW** (docs/conductor + 0120 mint; federated 5000-file budget warnings on operator `output/inc0102784-post-0107`).

### 2.7 How this advances the north star

Not UI polish: a receiving Relativity/DISCO load of DAT+OPT must reconstruct document boundaries from OPT `Y` breaks and Bates spans that match DAT `BEGBATES`/`ENDBATES`. A folder named `IMAGES/` with JPEGs, a multi-page TIFF, or `BEGBATES=ENDBATES` on a 3-page PDF **fails the track**. Unique-pst CLI is unchanged.

### 2.8 Last-PR Cursor comments (mandatory)

Last 4 merged: **#120** (docs 0114 Completed), **#119** (0114 product), **#118** (docs 0113 Completed), **#117** (0113 product).

| PR | Surface | Disposition |
|---|---|---|
| **#120** | docs registry | none |
| **#118** | docs registry | none |
| **#117** | `produce.rs` (host + UI) | three items already **0119**. Do not steal. |
| **#119** | `review_window.rs` / `produce.rs` UI | three **valid** Bugbot items, **not this track** — **minted 0120**: (1) High — mouseup `offsetX`/`offsetY` vs overlay child records the wrong box; (2) Medium — `drawing` / drag origin survive doc/page change; (3) Medium — Burn counts prefer stale QC snapshot after `produce_burn_set`. Live-verified 2026-08-30 on `review_window.rs` ~1110–1137 and 373–381, `produce.rs` ~367–421. |

### 2.9 Crate boundaries

| Crate | This track |
|---|---|
| `pdf-raster` | TIFF sniff + inbound TIFF decode; `DPI_PRODUCE`; 1-bit + G4 encode + Bates endorse; `NativeKind::Tiff`. **Not** a new workspace crate unless G4 wrapping cannot live here. |
| `matter-produce` | `IMAGES/` layout, OPT writer, page-level sequence, DAT beg/end span, image profile packaging. |
| `matter-core` | Schema v41 (`production_items` columns + `production_image_pages`); profile body extras (`include_images`, `bates.mode`, `layout.images`); builtin image profile. **No** chrome `connection()` SQL. **Prefer not** to append `ITEM_COLUMNS`. |
| `matter-qc` | New pack `qc_image_opt_v1` + image rules. Do **not** Error-promote those rules on `qc_default_v1`. |
| `dedupe-chrome` | Format + Number wiring; drop “ships in 0115” copy when the image profile is selected. Image-tab TIFF preview (first page / page nav). |
| `dedupe-desk` / `process-runner` | **Do not depend.** Do not fold Process. CLI `produce run` already passes `production_profile` — engine work covers headless. |

### 2.10 Review fold-in (2026-08-30)

Sources: `opencode-review.md`, `agy-review.md`. Harness files **not** edited. Live-verified: `fax-0.3.0/src/tiff.rs` wrap tags (Compression=4, Photometric=0, **X/YRes=200**, no 258/266/277); `advance_next_seq_for_control` parses a **single** control (`run.rs` ~1486–1490); `MAX_PAGES=500` (`extract-pdf/src/limits.rs`, `pdf-raster` ~283); `sanitize_filename_part` does **not** strip comma/space (`layout.rs` ~273–280); `production_profile_config_hash` is SHA-256 of `serde_json::to_string(body)`.

| Id | Disposition |
|---|---|
| **opencode-M1** / **agy-M1** | **Fold** — explicit IFD is the produce artifact; wrap forbidden. agy-M1 overstated “omitted resolution tags” (wrap writes 200 dpi); the defect is **wrong dpi + missing BitsPerSample**, not 72-dpi default. DoD-2 asserts tag **presence** of BitsPerSample=1 and XRes=`image_dpi`. |
| **opencode-M2** | **Fold** — `image_folder_cap ≥ pdf-raster::MAX_PAGES`; overflow = item error, never split. |
| **agy-M2** / **opencode-m4** | **Fold** — resume is a **code change** to `advance_next_seq_for_control` (today parses beg only). Crash-mid-document test. |
| **agy-M3** | **Already covered** in §3.3; strengthen DoD/Phase 1 inbound 2-IFD test. |
| **opencode-m1** | **Fold** — encoder recipe with `Encoder` / `encode_line` / `VecWriter` / `finish` names. |
| **opencode-m2** | **Fold** — TIFF Image tab is decode 1:1 + `LONG_SIDE_CAP`, not `DPI_REVIEW`. |
| **opencode-m3** | **Fold** — `image/tiff` decode-only; `ui/` must not gain fax/tiff/zpdf; 300 dpi is intended, `LONG_SIDE_CAP` may honestly downscale. |
| **opencode-m5** | **Fold** — OPT/span oracles read persisted `production_image_pages` / `production_items`. |
| **agy-m1** | **Fold** — OPT volume field strips comma and whitespace (live `sanitize_filename_part` does not). |
| **agy-m2** | **Fold** — BT.601 luma + threshold **160** (named constant). |
| **agy-O1** | **Already covered** — 0.25 in + solid box in §3.6; keep the lock. |
| **opencode-O1** | **Fold** — config-hash churn note (serde new keys). Not a DoD. |
| **opencode-O2** | **Decline** as work — pin count drift noted in §2.6. |

---

## 3. In scope

### 3.1 Placement

| Component | Location |
|---|---|
| G4 encode + **explicit IFD** + Bates stamp | `pdf-raster` (`g4.rs` or sibling). `fax::tiff::wrap` is not the produce path. |
| OPT + `IMAGES/` + page Bates | `matter-produce` (`opt.rs`, `layout.rs`, `run.rs`) |
| Schema v41 + image profile | `matter-core` |
| Image QC pack | `matter-qc` |
| Chrome Format/Number + TIFF Image-tab | `dedupe-chrome` |
| Fixtures | `tempfile` synthetic PDF (2 pages) + tiny JPEG + tiny xlsx/csv; optional `fixtures/tiff/` if tiny and synthetic |

### 3.2 Schema v41 (normative)

**Do not** append `ITEM_COLUMNS` for this track. Page counts are derived at produce/QC from `pdf_page_count`, inbound TIFF IFD count, or `raster_page(…).page_count`. JPEG/PNG = 1.

#### 3.2.1 `production_items` columns (ALTER)

| Column | Type | Notes |
|---|---|---|
| `end_bates` | TEXT NULL | Last page Bates. NULL on DAT-only rows (treat as `control_number`). |
| `page_count` | INTEGER NULL | Image pages written (0 = native-only). NULL on DAT-only. |

Keep `control_number` = **BEGBATES** (first page). Unique index `(production_set_id, control_number)` **stays** — do not put interior pages in this table.

#### 3.2.2 `production_image_pages`

| Column | Type | Notes |
|---|---|---|
| `production_set_id` | TEXT NOT NULL | FK |
| `item_id` | TEXT NOT NULL | |
| `page_index` | INTEGER NOT NULL | 0-based |
| `bates` | TEXT NOT NULL | Page-level Bates (filename stem) |
| `relpath` | TEXT NOT NULL | Windows-style `IMAGES\001\PROD000001.TIF` |
| `sha256` | TEXT NOT NULL | Produced TIFF bytes |
| PRIMARY KEY | `(production_set_id, item_id, page_index)` | |
| UNIQUE | `(production_set_id, bates)` | |

#### 3.2.3 Production profile body (serde defaults; bump `PRODUCTION_PROFILE_BODY_VERSION` only if deserialize of v1 bodies would break — prefer additive defaults)

| Field | Default | Image builtin |
|---|---|---|
| `packaging.include_images` | `false` | `true` |
| `bates.mode` | `"document"` | `"page"` |
| `layout.images` | `"IMAGES"` | `"IMAGES"` |
| `packaging.image_dpi` | `300` | `300` |
| `packaging.image_folder_cap` | `500` | `500` |

**Invariant:** `image_folder_cap ≥ pdf-raster::MAX_PAGES` (live **500**). Profile upsert / builtin validation **rejects or normalizes** a smaller cap (do not silently split). Additive serde keys change `production_profile_config_hash` (SHA-256 of `serde_json::to_string(body)`) for the DAT-only builtin once at v41 — benign (QC fingerprints key ids+pack, not this hash); do not treat that shift as profile drift on resume.

New reserved builtin slug: **`us_concordance_image_opt_v1`** (label: US Concordance + TIFF G4 / OPT). Bound QC pack **`qc_image_opt_v1`**. Same DAT field map as `us_concordance_native_text_v1`. `filename_mode` remains `name_by_bates`.

DAT-only builtin **unchanged**. Chrome default profile **unchanged**.

### 3.3 Image-eligible vs native-only

| Kind | Image? | Notes |
|---|---|---|
| PDF (`detect_pdf` / `%PDF`) | **Yes** | Raster **burned** native when 0114 burn-required; else original. Encrypted → existing `pdf_encrypted` / produce error, not a blank TIF. |
| JPEG / PNG | **Yes** | Decode → 1-bit G4 (colour discarded). One page. |
| TIFF inbound | **Yes** | Decode each IFD; emit **one single-page G4 TIF per IFD**. Do not ship the original multi-page TIFF as the image artifact. |
| EML / MSG / RFC822 | **No** | Native in `NATIVES/`. DAT `BEGBATES=ENDBATES`. QC **Warn** `image_skipped_native_only`. Image tab may keep the 0114 empty copy or say “Native-only (no print-to-TIFF).” |
| XLS / XLSX / CSV / TSV | **No** | Same. Placeholder promised “natives for xls/xlsx/csv remain.” |
| DOCX / PPTX / other OOXML | **No** | Same. Residual print/render. |
| Other / missing native | **No** | Existing missing-native / synthetic EML rules unchanged. |

Do **not** invent slip-sheet TIFFs (**D-0040-10** stays residual).

### 3.4 Page-level Bates (image profile only)

Assignment walks the **same** family-together ordered id list as 0113 (`order_ids_family_together`, `expand_family=false` from chrome).

For each produced (non-withheld) item:

1. `page_count` = image pages (§3.3) or **0** (native-only).
2. If `page_count == 0`: `control = format(prefix, next_seq)`; `end_bates = control`; `next_seq += 1` (document-level, same as today).
3. If `page_count >= 1`: `beg = next_seq`; `end = next_seq + page_count - 1`; `control_number = format(beg)`; `end_bates = format(end)`; write pages `beg … end`; `next_seq += page_count`.

DAT: `BEGBATES=control_number`, `ENDBATES=end_bates` (may differ), `CONTROL_NUMBER=control_number`.

Resume: never renumber a prior-ok row. Live `advance_next_seq_for_control` (`run.rs` ~1486–1490) parses **one** control and walks `seq+1` — that is **beg-only** today. Image resume **must** pass `end_bates.as_deref().unwrap_or(control_number)` (or equivalent) so a crash after page 1 of a 3-page item cannot reuse interior Bates. This is a **code change**, not a config flip. CI: crash-mid-document fixture (page 1 of 3 written, resume, next item’s beg = old end+1).

Document-level DAT-only profile: **do not** change `next_seq += 1` behaviour.

### 3.5 Volume layout (image profile)

```text
<vol>/
  DATA/load.dat          # UTF-8 BOM þ/¶/® — unchanged dialect
  DATA/load.csv          # twin if profile says so
  NATIVES/<BEGBATES>.<ext>
  TEXT/<BEGBATES>.txt
  IMAGES/001/<PAGEBATES>.TIF
  IMAGE.opt              # volume root, CRLF
  README.txt             # mention IMAGES + IMAGE.opt when present
  privilege-log.csv      # 0113 location; ControlNumber = BEGBATES
```

- Folder shard: `IMAGES\{001,002,…}` with **`image_folder_cap`** TIFFs max (builtin **500**). A multi-page document **must** stay in one folder — if the current folder cannot fit `page_count` remaining slots, start that document in the **next** folder. If `page_count > image_folder_cap` (should be unreachable when cap ≥ `MAX_PAGES`), **fail-closed**: item error, do **not** split pages across folders.
- Filename = page Bates + `.TIF` (uppercase ext locked). No spaces. `sanitize_filename_part` already used for natives.
- **Do not** write `IMAGES/` or `IMAGE.opt` on the DAT-only profile (0113 DoD remains: those paths must not exist).
- OPT volume field (field 2): production stamp / volume folder name, then **strip comma and whitespace** (and apply `sanitize_filename_part`). Live `sanitize_filename_part` does **not** remove `,` or space — a dedicated OPT token is required so comma-delimited parsers do not split the field.

`IMAGE.opt` writer:

- One line per **image page** (not per native-only item).
- Field 4 `Y` iff `page_index == 0`.
- Field 7 page count iff `page_index == 0`.
- Interior: empty fields 4–7.
- CRLF. No BOM. No quotes.

### 3.6 Bates endorsement

Before G4, paint a **legible Bates** in the lower-right of the raster (solid white/black **background box** + 5×7 monospace digits — **no** new font crate, **no** GPL fonts). Dedicated stamp bounding box with a **≥ 0.25 in** margin from the page edge. Stamp is on the **image**, not the native. Highlights still never burn.

DoD fixture: OCR-not-required; assert the Bates **string appears as pixels** in a known corner (decode G4 → bitmap; count black pixels in the stamp region, or round-trip decode and sample). Simpler: decode produced TIF and assert the stamp region is not uniformly white.

### 3.7 Chrome

**Format step:** drop the 0115-off copy when `include_images` profile is selected. Enable a real control: profile dropdown already exists — add builtin `us_concordance_image_opt_v1`. Helper copy: “Single-page TIFF G4 + IMAGE.opt. Spreadsheets and email stay native-only. LFP is not this track.”

**Number step:** enable **Page-level Bates** only when the selected profile has `bates.mode=page` (image builtin). Selecting page-level Bates **selects the image profile** (do not allow page-level Bates on DAT-only). Selecting DAT-only profile forces document-level Bates. Family-together stays locked.

**Burn step:** unchanged 0114 contract (do not fix 0120 stale counts). Image produce still fail-closes on `burned_native_missing`.

**Image tab:** sniff TIFF → decode **native pixel size 1:1**, then apply existing `LONG_SIDE_CAP` (same as JPEG/PNG). **Do not** pass `DPI_REVIEW` (that scale is for zpdf PDF `render_display_list` only). Multi-IFD: page nav `,`/`.` . Empty copy for EML stays honest (names native-only, not “0115” forever). **Do not** implement 0120 draw-coordinate bugs.

**Finalize:** still `fail_if_withheld=true`, `require_qc_pass=true`, same `scope=item_ids`. Image profile → QC pack `qc_image_opt_v1`. Do not steal 0119 Finalize re-arm.

Workers: `join_worker` + `std::thread::spawn`. Caps + `catch_unwind` per document. DoD fixture is small; multi-GB cancel stays **D-0113-long-job** / **0116**.

### 3.8 Produce / QC extras

New pack **`qc_image_opt_v1`**: default pack **plus**:

| id | default on image pack | when |
|---|---|---|
| `image_page_missing` | **Error** | image-eligible item with `page_count` predicted ≥ 1 and (after produce, or preflight if pages already known) missing TIF / OPT row |
| `beg_end_bates_span` | **Error** | `end_seq - beg_seq + 1 != page_count` when `page_count ≥ 1` |
| `opt_row_count_mismatch` | **Error** | OPT lines ≠ sum of image `page_count` (engine self-check at finalize; also a unit test). Pre-produce: skip if OPT not written yet — this rule is **post-volume or produce-internal**, not a 0041 preflight Error that can never pass. Prefer: **produce-internal fail-closed** + a preflight rule only when a prior incomplete volume is being resumed. Do **not** add a preflight Error that fires because OPT does not exist yet. |
| `image_skipped_native_only` | **Warn** | xls/xlsx/csv/eml in an image-profile set |
| `multi_page_tiff_as_artifact` | **Error** | produced image file has more than one IFD |

Keep 0114 Errors on this pack too (`burned_native_missing`, `text_redact_unmapped_on_pdf`).

**`qc_default_v1` is unchanged.** DAT-only fingerprints must not go stale because of image rules.

**Source of truth:** `beg_end_bates_span` and the OPT-line count self-check read **persisted** `production_items.page_count` / `end_bates` and `production_image_pages` rows — not a live re-raster. A partial-write window must not make both checks pass while the volume is wrong.

Produce-internal fail-closed (even without QC): image profile + image-eligible + zero TIFFs written → job error, volume not `complete`. OPT/DAT Bates span mismatch → job error.

### 3.9 Image tab TIFF (review)

`pdf-raster::sniff_kind`: magic `II*\0` / `MM\0*` → `NativeKind::Tiff`. Decode page via `tiff` / `image` **tiff** feature (decode-only). PNG for the tab at **native resolution**, capped at `LONG_SIDE_CAP`. Multi-IFD: page nav `,`/`.` . Corrupt TIFF → honest error, not hang.

G4 production still **re-encodes** each IFD as its own single-page G4 TIF; never copy a hostile inbound TIFF into `IMAGES/`.

### 3.10 Hygiene

- Production: no `unwrap` / `expect`. `main` still `Result`.
- Never mutate source PSTs / Purview. Never commit client TIFFs, `output/`, `evidence/`.
- Tests: `tempfile` + `insert_family` → parent → children; `ensure_default_review_set`; `put_bytes` for a 2-page uncompressed PDF whose burned/original raster is deterministic. No client evidence.
- `fax` MIT must pass `cargo deny`. New LicenseRef-Proprietary only if a new crate is created (prefer not).
- `ui/` stays excluded. DoD-5 asserts `crates/dedupe-chrome/ui/Cargo.toml` gains **no** `fax` / `tiff` / `zpdf`. G4 encode is host-only.
- 300 dpi is **intended**. `LONG_SIDE_CAP` (4096) may honestly downscale a page whose long side exceeds ~13.9 in at 300 dpi (`truncated=true`); do not lie in tags — IFD X/YRes still records `image_dpi`, or if the bitmap was downscaled, record the **effective** dpi (`image_dpi * long_cap / native_long`) so the file is geometry-honest. Prefer: intended 300; if truncated, set X/YRes to the effective value.

---

## 4. Out of scope (do NOT do here)

- **0120** PR #119 UI Bugbot (wrong overlay coords, draw state across pages, stale Burn counts).
- **0119** Finalize re-arm / empty privilege-log `filter_ids` / QC across matters.
- **0117** queue header/spacer / vacant lie / arrow scroll.
- **0118** `review_document` / `review_document_body` stale fetch.
- **0116** process-runner on chrome / egui Process fold / multi-GB cancel.
- Desk produce-image UX (engine profile is enough for CLI/Desk-later).
- LFP / DII / EDRM XML (**D-0115-lfp** residual).
- Colour JPEG image pages (**D-0115-color**).
- Email / OOXML print-to-TIFF (**D-0115-email-print**).
- Slip sheets (**D-0040-10**).
- Opposing OPT ingest (**D-0042-06**).
- Whole-document PDF as the image format (TIFF G4 only).
- Multi-page TIFF **output**.
- Inverse redact, Office native redact, pdf.js, MuPDF, Poppler, GPL pdfium, wgpu.
- Unique-pst flags, BCC-default, schema beyond v41.
- Vendoring the mock. Axum daemon. Leptos SSR. tauri 3.x. leptos 0.9.
- Chrome `connection()` SQL. Extending `ReviewListRow`.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0114 on main (HEAD at plan-time `64ae7f2` docs / product `5ed53bf`). Schema **40**. `pdf-raster::raster_page` + `resolve_native` burn-required.
- **P2:** `fax` **0.3.x** still MIT; `Encoder` / `encode_line` / `VecWriter` / `finish` still exist. Produce artifact is an **explicit IFD**, not `fax::tiff::wrap`. If G4 encode is impossible without a denied license, **stop**.
- *Verified to date:* DAT `BEGBATES=ENDBATES`; no `IMAGES` layout; chrome Format/Number copy names 0115; `NativeKind` has no Tiff; PR #119 three UI items live; `qc_default_v1` already has burned-native Errors.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| `tiff` crate cannot encode G4 | `fax` bitstream + explicit IFD; verify Compression=4 + BitsPerSample=1 + XRes=300 in DoD. |
| `fax::tiff::wrap` as produce artifact | **Forbidden.** Wrap is 200 dpi and omits BitsPerSample. Explicit IFD is primary. |
| Page Bates collides on resume | Change `advance_next_seq_for_control` to honor **end_bates**; unique on `production_image_pages.bates`; crash-mid-doc test. |
| Image rules break DAT-only QC | New pack only; default pack unchanged. |
| Colour / photo pages go unreadable at 1-bit | Accepted for v1; residual **D-0115-color**. |
| EML “image set” incomplete | Honest native-only + Warn; residual print. |
| 300 dpi × 500 pages hangs chrome | Caps, per-doc `catch_unwind`, DoD small; residual **0116**. |
| Bates stamp covers redacted tokens | Lower-right margin; DoD token-gone still holds on burned source. |
| Overlay Bugbot mis-burns then we image the wrong pixels | **0120** owns draw coords; this track rasters **stored** geom/burned CAS, not live drag. |
| Folder split mid-document | Cap ≥ `MAX_PAGES`; overflow = item error, never split. |
| OPT comma in volume field | Strip `,` and whitespace; `sanitize_filename_part` is not enough. |
| Config hash churn on DAT-only builtin | Additive serde keys; one-time hash shift; not QC fingerprint. |
| `ITEM_COLUMNS` shift | Do not append item columns. |
| License drift | `cargo deny`; pin `fax` in CHANGELOG. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — DAT-only unchanged:** Produce with default `us_concordance_native_text_v1` still writes `DATA/load.dat` + `NATIVES/` + `TEXT/` and **does not** create `IMAGES/` or `IMAGE.opt`. `BEGBATES=ENDBATES=CONTROL_NUMBER`. Existing 0113 chrome tests that assert no `IMAGES/` stay green.
- [ ] **DoD-2 — Image volume:** Image-profile produce of a **2-page** synthetic PDF (optionally post-0114 burn) writes `IMAGES\001\<beg>.TIF` and `IMAGES\001\<beg+1>.TIF`, plus `IMAGE.opt` with two lines: first `Y,,,2`, second empty breaks. Each TIF: little-endian `II*\0`, **one IFD**, **Compression=4**, **BitsPerSample tag = 1** (must be present — TIFF-default-1 is not enough for this oracle), **XResolution = YResolution = 300** (or the honest effective dpi if `LONG_SIDE_CAP` truncated), **ResolutionUnit=2**, Photometric=0. `DATA/load.dat` has `BEGBATES` ≠ `ENDBATES` (`end = beg+1`). `NATIVES/` still contains the (burned) PDF. Folder `IMAGES\001` exists; no multi-page TIF. **Inbound 2-IFD TIFF** (same volume or sibling test): two single-page G4 files, original multi-page TIFF **not** copied into `IMAGES/`. `fax::tiff::wrap` output (XRes=200, no tag 258) **fails** this DoD. **Resume:** crash after page 1 of a 3-page item, then resume — next item beg = that item’s end_bates+1 (no interior Bates reuse).
- [ ] **DoD-3 — Native-only honesty:** Same volume includes an xlsx/csv (or EML) sibling: DAT row with `BEGBATES=ENDBATES`, file under `NATIVES/`, **zero** OPT lines for that item. QC Warn `image_skipped_native_only` (not Error). No slip-sheet TIF.
- [ ] **DoD-4 — Token / burn:** If the PDF still requires 0114 burn, image produce **refuses** without a fresh burned native (`burned_native_missing`). After burn, G4 bytes and a decode of the TIF do **not** contain `SECRET_TOKEN_0114` (UTF-8 and UTF-16LE). Original CAS still does. Highlights do not appear as image content.
- [ ] **DoD-5 — Schema + CI:** `SCHEMA_VERSION == 41`. `production_items.end_bates` / `page_count` + `production_image_pages` exist. Builtin `us_concordance_image_opt_v1` reserved. `qc_default_v1` rule set **unchanged**. `cargo test -p pdf-raster` + `cargo test -p matter-produce` + `cargo test -p matter-qc` + `cargo test -p matter-core` + `cargo test -p dedupe-chrome` + `cargo check -p dedupe-desk`. Workspace fmt/clippy/test + `chrome-ui` trunk stay green. `cargo deny` accepts `fax` MIT. No production `unwrap`/`expect`. No `process-runner` on `dedupe-chrome`. `crates/dedupe-chrome/ui/Cargo.toml` has **no** `fax` / `tiff` / `zpdf`. Resume crash-mid-document test green.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0040-01` closed; `D-0060-04` closed for OPT profile (LFP residual **D-0115-lfp**); TIFF half of `D-0032-01` / `D-0030-01` closed; ledger committed (`FEATURE`). **0116** / **0117** / **0118** / **0119** / **0120** stay Proposed.

**Owner HITL (not CI):** release EXE, image profile, 2-page synthetic PDF + one spreadsheet, Finalize, open OPT/TIF/DAT. INC* waived.

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p pdf-raster
cargo test -p matter-produce
cargo test -p matter-qc
cargo test -p matter-core
cargo test -p dedupe-chrome
cargo check -p dedupe-desk
# rustup target add wasm32-unknown-unknown
# trunk build --config crates/dedupe-chrome/ui/Trunk.toml --release
```

---

## 9. Deferred (absorb / decline)

| Row | Disposition |
|---|---|
| **D-0040-01** TIFF/PDF image + OPT/LFP | **Absorb / close** for TIFF G4 + Opticon OPT. PDF-as-image-format and LFP **decline** (residuals). |
| **D-0060-04** Image + OPT/LFP profiles | **Absorb / close** for OPT profile `us_concordance_image_opt_v1`. LFP residual **D-0115-lfp**. |
| **D-0032-01** TIFF G4 half | **Absorb / close** remaining image-G4 half (PDF+jpeg/png burn already 0114). |
| **D-0030-01** Image box leftover | **Close** TIFF leftover; geometric boxes already 0114. |
| **D-0040-10** slipsheets | **Decline.** |
| **D-0042-06** opposing OPT ingest | **Decline.** |
| **D-0032-02** Office native redact | **Decline.** |
| **D-0115-lfp** (mint) | IPRO LFP writer — residual, not default. |
| **D-0115-color** (mint) | Colour JPEG image pages — residual. |
| **D-0115-email-print** (mint) | EML/OOXML print-to-TIFF — residual. |
| **D-0113-long-job** | Remain / **0116**. |
| **D-0114-pdfium-sidecar** / **D-0114-xform-text** | Remain. |
| **D-0117** / **D-0118** / **D-0119** | Remain. |
| **D-0120-pdf-raster-ui** (mint) | PR #119 three UI items. Remain Proposed. |
| **D-0116-process-fold** | Remain. |
| **D-0062-codesign** | Release ops. **Decline.** |
| **D-0108-keepset-crc-retaint** | Unique-export. **Decline.** |
| Last-PR #119 three UI items | **Minted 0120.** |
| Last-PR #117 three produce items | **0119.** |
| Mock coral / pdf.js / fake IMAGES | **Decline.** |
| opencode-M1 / agy-M1 wrap IFD | **Folded** — §2.5 explicit IFD; wrap forbidden. |
| opencode-M2 folder cap | **Folded** — §3.2.3 / §3.5. |
| agy-M2 / opencode-m4 resume | **Folded** — §3.4 code change + crash test. |
| agy-M3 multi-IFD | **Already covered** — DoD-2 inbound 2-IFD. |
| opencode-m1..m5 / agy-m1 m2 / O1–O2 / agy-O1 | **Folded or declined** as §2.10. |

---

## Series O index (do not reorder)

| ID | Item | After this plan |
|---|---|---|
| **0110** | Matter chrome + one overview command | **Completed** (PR **#111** / `5a76f0b`) |
| **0111** | Virtualized first-pass queue | **Completed** (PR **#113** / `3c4ca65`) |
| **0112** | Three-pane review window | **Completed** (PR **#115** / `81a3aad`) |
| **0113** | Produce checklist; DAT only | **Completed** (PR **#117** / `f192b2d`) |
| **0114** | zpdf raster + geometric redact | **Completed** (PR **#119** / `5ed53bf`) |
| **0115** | TIFF G4 + OPT | **Completed** (PR **#121** / `19d0c1f`) |
| **0116** | Fold egui Process | Proposed |
| **0117** | Queue virtualization residuals (PR #113) | Proposed |
| **0118** | Review-window async residuals (PR #115) | Proposed |
| **0119** | Produce-checklist residuals (PR #117) | Proposed |
| **0120** | Pdf-raster UI residuals (PR #119) | **Proposed — placeholder** |
