# ocr-plugin

Opt-in **offline OCR** for Dedupe Desk (track **0036**).

## What it does

- Job kind **`ocr`**: resumable, checkpointed, cancel between items
- Candidates: `pdf_needs_ocr=1` PDFs + image natives (png/jpeg/tiff/webp)
- Success: OCR plain text → CAS; sets review `text_sha256`; clears `pdf_needs_ocr`; clears FTS + redacted-text bookkeeping
- Default engine: **system Tesseract CLI** with **`--psm 1`** (OSD / orientation detection)
- CI: **`MockOcrEngine`** — no system Tesseract required

## Operator install

OCR is **off by default**. Core Desk builds and runs without Tesseract.

1. Install [Tesseract](https://github.com/tesseract-ocr/tesseract) (Apache-2.0) for your OS.
2. Ensure **`osd`** traineddata is present (often package `tesseract-ocr-osd` or full tessdata).
3. For **PDF OCR**, also install a page renderer:
   - Poppler **`pdftoppm`**, or
   - MuPDF **`mutool`**
4. In Desk **Settings**:
   - Check **Enable local OCR**
   - Optionally set paths if tools are not on `PATH`
5. Workspace → **Run OCR**

License note: operators install Tesseract separately; Desk does not bundle the binary in P0.

## Limits (P0)

| Limit | Value |
|---|---|
| Max native | 100 MiB |
| Max pages | 500 |
| Max OCR text | 10 MiB (truncation marker) |
| Default DPI | 200 |
| Default PSM | **1** (with OSD) |
| Concurrency | 1 page at a time per item |

Temps live under `<matter>/workspace/temp/ocr/` with **Drop guards** and a **startup purge**.

## Params (job JSON)

```json
{
  "force": false,
  "batch_size": 20,
  "lang": "eng",
  "max_pages": 500,
  "dpi": 200,
  "enabled": true,
  "tesseract_path": null,
  "tessdata_dir": null,
  "pdf_renderer_path": null,
  "engine": "tesseract"
}
```

- `enabled: false` → job fails immediately; no item mutation
- `engine: "mock"` → tests only
- `force: true` → re-OCR prior successes

## Tests

```powershell
cargo test -p ocr-plugin
# Optional live smoke (requires system Tesseract + osd):
# cargo test -p ocr-plugin -- --ignored
```
