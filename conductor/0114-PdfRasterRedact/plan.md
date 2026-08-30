# 0114 — PdfRasterRedact — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> **Ledger (implement):** `ledgerful ledger start crates/pdf-raster --category FEATURE --message "0114 zpdf raster + geometric PDF burn"` — commit in the final phase.
>
> Planning tx (Ready pass): `3f43e5c4-7fb0-4adb-aa47-b1524d870a02`.
> Fold-in tx: `57006a8f-7bf4-4c00-8f3b-0366c89fc34f` (`opencode-review.md` + `agy-review.md`; spec §2.10).

---

## Phase 0 — Precondition / pin gate → DoD-1, DoD-5

- [ ] Re-verify `SCHEMA_VERSION` **39**, `item_redactions` text-only, `resolve_native` copies `native_sha256`, Image stub copy, produce Burn copy, extract-pdf caps (100 MiB / 500 pages), CSP `img-src 'self' data:` (spec §2.2).
- [ ] Re-verify crates.io `zpdf` **0.13.x** MIT, feature **`cpu-render`** (default). Confirm:
  - `ContentInterpreter::new(page.effective_box()).with_page_rotation(page.rotate)` → `DisplayList`
  - `CpuRenderer::render_display_list` → `RenderedPage` **RGBA** (PNG is a separate `image` encode)
  - `IncrementalWriter::redact_page` + `write` + `PdfFile::parse` + `rewrite_pdf(&PdfFile, &RewriteOptions)`
  - `iw.document()` does **not** include pending edits
  - `search_spans` line-bounded
- [ ] If there is **no** rewrite API, **stop** — incremental-only burn is forbidden.
- [ ] Re-verify tauri **2.x** + leptos **0.8**. Keep `ui/` workspace **exclude**. Keep `chrome-ui`.
- [ ] Do **not** vendor `C:\dev\dedupe-frontend`. Do **not** add wgpu/pdf.js/MuPDF/Poppler/GPL `pdfium`. Do **not** implement **0117** / **0118** / **0119**. Do **not** add `process-runner` to chrome.
- [ ] Do **not** depend on `dedupe-desk`. Copy `detect_pdf` usage from `extract-pdf` (path dep OK).

## Phase 1 — Schema v40 + engine crate → DoD-2, DoD-3, DoD-5

- [ ] `SCHEMA_VERSION = 40`. Table `item_geom_redactions`. **Append** item columns `geom_redaction_count`, `burned_native_sha256`, `burned_native_at`, `burned_source_digest`, `raster_engine` at the **end** of `ITEM_COLUMNS` / `map_item_row` (do **not** insert after `redacted_source_digest`).
- [ ] Fingerprint = `native_sha256` + canonical active geom + **0032 text state** + engine pin. Native/geom/text change → stale.
- [ ] Matter helpers: create/list/delete geom; host pixel→user-space map (CropBox + `/Rotate` + y-flip); audit on create/delete/burn.
- [ ] New workspace crate `pdf-raster`: DisplayList + RGBA→PNG; burn compose §2.4; caps + `catch_unwind`. JPEG/PNG decode via `image`. JPEG **burn re-encodes JPEG**. Encrypted PDF fail-closed. No unwrap in production. **Do not** enable `gpu-render`.
- [ ] Tests: uncompressed PDF with `SECRET_TOKEN_0114`. Overlay does not rewrite native; burn changes digest; token gone from burned bytes (UTF-8 + UTF-16LE) and extract; original CAS still has it; `iw.document()` shortcut would fail this oracle. **Variant:** burn, add a second 0032 redaction, produce refuses until re-burn. `/Rotate 90` (or CropBox) box-on-visible-token. JPEG burn `FILE_EXT=jpg` with JPEG magic.
- [ ] `matter-produce::resolve_native`: if burn required, copy `burned_native_sha256` only; missing/stale → `burned_native_missing`. Never fall back to original. Extension/mime from burned payload.
- [ ] `matter-qc`: `RULE_BURNED_NATIVE_MISSING` + `RULE_TEXT_REDACT_UNMAPPED_ON_PDF` **Error** on default pack (same shape as `RULE_REDACTED_TEXT_MISSING`).

## Phase 2 — Chrome Image tab + Burn → DoD-1, DoD-4

- [ ] Host commands on `join_worker` (spec §3.7). **Mint** Image-tab `raster_generation` (bump on `doc_id` and page change); pass `item_id` + generation; discard mismatches. Do **not** fix 0118 document/body fetches.
- [ ] Image tab: PNG + overlay hatch; draw/delete; full-page; from-hits (`search_spans` + 1 pt dilation; zero-hit honesty); page nav `,`/`.` ; drop stub copy. EML empty copy names **0115**. `r` focuses live tab. Do not steal `[` `]`.
- [ ] Produce step 4: counts + Burn set button; drop “Geometric PDF burn is 0114”; keep “Highlights never burn.” QC Errors surface as blockers.
- [ ] JPEG/PNG: display + paint-burn with matching codec.

## Phase 3 — CI + docs → DoD-5

- [ ] `cargo fmt` / `clippy -D warnings` / `cargo test --workspace` / `cargo test -p pdf-raster` / `cargo test -p dedupe-chrome` / `cargo test -p matter-core` / `cargo test -p matter-qc` / `cargo test -p matter-produce` / `cargo check -p dedupe-desk` / trunk `ui/`.
- [ ] `cargo deny` (zpdf MIT). Record pin in CHANGELOG.
- [ ] CHANGELOG Unreleased. Close `D-0032-01` / `D-0034-02` / `D-0034-04` per spec §9. Leave **0115–0119** as-is. Optional residual `D-0114-pdfium-sidecar` / `D-0114-xform-text`.

## Phase 4 — Finalize → DoD-6

- [ ] Owner HITL: **release** EXE. Synthetic PDF, draw box, Burn, Finalize DAT, produced native has no token. INC* waived.
- [ ] `review.md`; `../conductor.md` + `sequencing.md` + `ROADMAP.md`: **0114 Completed**.
- [ ] Commit the ledger transaction.
- [ ] **0115** still parked. **0116** / **0117** / **0118** / **0119** still Proposed.

---

## Handoff notes

- Burn is outward-facing: a produced native that still contains the secret is a **defect**, not a known_gap.
- Rollback: unused `burned_native_sha256` blobs are CAS garbage (harmless); original natives remain.
- Single-exe / no-daemon remains. No user-managed pdfium unless they drop a DLL next to the EXE (optional, not DoD).
- Re-verify zpdf APIs at execute — method names are **plan-time 0.13.0**.
