# extract-pdf

Pure-Rust **PDF embedded-text extraction** for Dedupe Desk (track **0034**).

Fills `text_sha256` from PDF natives already in matter CAS — **no PDFium**, no
Poppler, no cloud OCR, **no page rasterization** in P0.

## Method stack

| Role | Crate | Pin |
|---|---|---|
| Text operators | **pdf-extract** | **0.12.0** |
| Structure / encrypt / page map | **lopdf** (transitive via pdf-extract) | **0.42.x** |

Method id: `pdf_extract_v1`.

> Plan pin mentioned lopdf 0.44.x; **pdf-extract 0.12.0** depends on **lopdf 0.42**.
> We use the transitive lopdf via `pdf_extract::Document` to avoid dual versions.

## Safety limits

| Limit | Default |
|---|---|
| Max native input | 100 MiB |
| Max pages processed | 500 |
| Max extracted text | 10 MiB |
| Min text chars (total non-ws) | 50 |
| Min text chars / page | 20 |

Truncation marker:

```text
\n[… truncated …]\n
```

Encrypted PDFs fail closed (`pdf_encrypted`). Corrupt bytes → `pdf_parse_error`
(or isolated panic → same). Native size over cap → `pdf_limit_exceeded`.

## Empty / low-text / needs OCR

| Condition | `pdf_extract_status` | `pdf_needs_ocr` | Text CAS |
|---|---|---|---|
| Zero non-whitespace chars | `empty` | **1** | NULL |
| Below total or per-page threshold | `low_text` | **1** | **written** |
| Above both thresholds | `ok` | **0** | written |

Whitespace-only does **not** count as enough text. OCR handoff is track **0036**
(candidates = `pdf_needs_ocr = 1`).

## Job: `pdf_extract`

| Item | Value |
|---|---|
| Kind | `pdf_extract` |
| Stage | `pdf_extract` |
| Params | `{ "force": false, "batch_size": 50 }` |

- Idempotent skip for terminal statuses `ok` / `low_text` / `empty` / `skipped`
  when `pdf_source_native_sha256 == native`
- Error status does **not** set source (retryable)
- Candidate paging uses a **stable** PDF-eligible list + OFFSET
- On text write: NULL `redacted_text_*` (0032); clear `fts_*` (0029)
- **Never** rewrites native CAS

## Error codes

| Code | Meaning |
|---|---|
| `pdf_not_pdf` | Missing `%PDF-` / not a PDF |
| `pdf_encrypted` | Password-encrypted |
| `pdf_parse_error` | Corrupt / parser panic isolated |
| `pdf_limit_exceeded` | Size / page / text cap |
| `pdf_empty_text` | Zero extractable text |

## API

```rust
use extract_pdf::{extract_pdf, run_pdf_extract, PdfExtractParams};

let extracted = extract_pdf(&bytes, Some("memo.pdf"), None)?;
// extracted.text, extracted.method, extracted.partial, extracted.class, extracted.page_count
```

Panic isolation for jobs: `extract_pdf_catch_unwind`.

## Fixtures

Synthetic only under `fixtures/pdf/` (marker `PDF_TEXT_MARKER`):

```powershell
cargo run -p extract-pdf --example gen_pdf_fixtures
```

## Out of scope (P0)

| Deferred | Owner |
|---|---|
| OCR for empty/low-text | **0036** |
| First-page / multi-page **raster preview** | residual optional **PDFium/MuPDF** — **not** pure-Rust |
| Geometric PDF redaction burn-in | residual (D-0032-*) |
| Password cracking | never |

## Blocking thread

`extract_pdf` / `run_pdf_extract` are CPU/IO bound — run only on the
process-runner matter worker, never on the GUI thread.
