# extract-office

Pure-Rust **OOXML text extraction** for Dedupe Desk (track **0033**).

Fills `text_sha256` from Office natives already in matter CAS — **no LibreOffice**,
no COM, no cloud convert.

## Formats & methods

| Format | Extensions (best-effort) | Method id | Engine |
|---|---|---|---|
| Word | `.docx` `.docm` | `docx_xml_v1` | zip + quick-xml (`w:t`) |
| Excel | `.xlsx` `.xlsm` | `calamine_xlsx_v1` | calamine 0.36 |
| PowerPoint | `.pptx` `.pptm` | `pptx_xml_v1` | zip + quick-xml (`a:t`) |

Legacy OLE (`.doc` / `.xls` / `.ppt`) → `unsupported_legacy_office` (no panic).

## Safety limits

| Limit | Default |
|---|---|
| Max native input | 100 MiB |
| Max zip entry uncompressed | **50 MiB** |
| Max inflate ratio | ~100:1 (when compressed size known) |
| Max zip entries | 10_000 |
| Max extracted text | 10 MiB |
| Max sheets / slides | 500 |

### Streaming entry rule (**required**)

**Every** zip entry is read with:

```rust
reader.take(MAX_UNCOMPRESSED_ENTRY_BYTES)
```

Never call unbounded `read_to_end` / `io::copy` on an entry stream. ZIP
`uncompressed_size` headers are **not trusted alone** (zip-bomb spoofing).

### XLSX early break (**required**)

Build output incrementally while iterating cells/rows. When
`output.len() >= MAX_EXTRACTED_TEXT_BYTES`, **stop immediately** — do not
stringify the whole workbook first. Truncation marker:

```text
\n[… truncated …]\n
```

## Job: `office_extract`

| Item | Value |
|---|---|
| Kind | `office_extract` |
| Stage | `office_extract` |
| Params | `{ "force": false, "batch_size": 50, "formats": ["docx","xlsx","pptx"] }` |

- Idempotent skip when `text_sha256` set and `office_source_native_sha256 == native_sha256`
- `force: true` re-extracts
- On text write: NULL `redacted_text_*` (0032); clear `fts_*` so 0029 re-indexes
- **Never** rewrites native CAS
- Cancel between items; checkpoint cursor

## Error codes

| Code | Meaning |
|---|---|
| `unsupported_legacy_office` | OLE / legacy extension |
| `encrypted_office` | Password-encrypted OOXML |
| `office_parse_error` | Corrupt zip/XML |
| `office_limit_exceeded` | Size / ratio / entry / text cap |
| `office_empty_text` | Parse ok but zero text |

## API

```rust
use extract_office::{extract_office, run_office_extract, OfficeExtractParams};

let extracted = extract_office(&bytes, Some("memo.docx"), None)?;
// extracted.text, extracted.method, extracted.partial
```

Panic isolation for jobs: `extract_office_catch_unwind`.

## Fixtures

Synthetic only under `fixtures/office/` (markers `OFFICE_DOCX_MARKER`,
`OFFICE_XLSX_MARKER`, `OFFICE_PPTX_MARKER`).

## Out of scope

PDF (**0034**), LibreOffice sidecar, native Office redaction, macro execution,
password recovery, WYSIWYG preview, full taxonomy (**0037**).
