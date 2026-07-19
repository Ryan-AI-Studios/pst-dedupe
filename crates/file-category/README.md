# file-category

Stable **`taxonomy_v1`** file-category classifier and resumable `classify` job
for Dedupe Desk (track **0037**).

## Vocabulary (`taxonomy_v1`)

| Category | Meaning |
|---|---|
| `email` | Message item (incl. standalone `.msg`) |
| `calendar` | Appointment / ICS event |
| `contact` | Contact card (thin) |
| `chat` | Chat / short message (stub for 0055) |
| `document` | Word-processing / plain text |
| `spreadsheet` | Tables / workbooks / CSV |
| `presentation` | Slide decks |
| `pdf` | Portable Document Format |
| `image` | Still images |
| `multimedia` | Audio / video |
| `archive` | Containers / compressed (not OOXML) |
| `database` | DB files |
| `log` | Log / event files |
| `executable` | PE / scripts treated as exec noise |
| `system` | OS/system noise (thin) |
| `pst` | Outlook data file inventory row |
| `mobile` | Mobile packages (thin) |
| `cloud` | Cloud package markers (thin) |
| `other` | Recognized file, no family match |
| `unrecognized` | No path/mime/magic signals |

**Forbidden as category:** `attachment` — reclassify content; keep `role=attachment`.

### Input aliases

| Alias | Maps to |
|---|---|
| `doc`, `docs`, `word` | `document` |
| `xls`, `xlsx`, `sheet` | `spreadsheet` |
| `ppt`, `slides` | `presentation` |
| `container`, `zip` | `archive` |
| `video`, `audio`, `media` | `multimedia` |
| `exe`, `binary` | `executable` |
| `unknown` | `unrecognized` |

## Classification priority

1. **Structural / message_class** — `IPM.Appointment*` → `calendar`; default message → `email`
2. **Extractor refine** — keep closed-set non-legacy when `respect_extractor_refine` (default non-force)
3. **Magic** (≤64 KiB CAS head) — specific magic (PDF/PNG/JPEG/PE) beats extension
4. **MIME** — stored or `mime_guess`
5. **Extension** — curated table including **`.msg` → email**
6. **Fallback** — `unrecognized` if no signals else `other`

### Container magic (§3.4.1)

| Detected | Behavior |
|---|---|
| **ZIP** | Do **not** force `archive`. OOXML peek (`[Content_Types].xml`, `word/`/`xl/`/`ppt/`) → document/spreadsheet/presentation (`magic_ooxml`). Else extension tie-break. |
| **OLE/CFB** | Do **not** one-bucket. `.msg`→`email`, `.doc`→`document`, `.xls`→`spreadsheet`, `.ppt`→`presentation`; unknown → `other` (not archive). |
| **Specific** (PDF, PNG, …) | Magic is decisive (e.g. `%PDF` + `invoice.docx` → `pdf`). |

## Job `classify`

```json
{
  "force": false,
  "batch_size": 100,
  "use_magic": true,
  "in_review_only": false,
  "respect_extractor_refine": true
}
```

Candidates: `file_category` NULL or in `{attachment, other, unrecognized}` or `force` or `category_taxonomy` NULL / ≠ `taxonomy_v1`.

Audit: `classify.start` / `classify.complete` / `classify.fail`.

Never mutates `role`, `parent_item_id`, CAS digests, or text.

## Dependencies

| Crate | Pin | Role |
|---|---|---|
| `mime_guess` | 2.0.x | Extension → MIME |
| `infer` | 0.19.x | Magic → MIME (pure Rust) |

**No** libmagic / system `file(1)`.

## Schema (matter-core v18)

Bookkeeping on `items`: `category_method`, `category_taxonomy`, `category_status`,
`category_error`, `categorized_at`. Filter/cull field remains `file_category`.

## Note for 0038

`GROUP BY file_category` is now meaningful for dashboard type rollups.
