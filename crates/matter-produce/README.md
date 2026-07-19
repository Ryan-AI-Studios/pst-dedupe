# matter-produce

Production export for Dedupe Desk (track **0040**): package a review set as **natives + text + Concordance load file**.

## Job

| Item | Value |
|---|---|
| Kind | `produce` (alias `production_export`) |
| Stage | `produce` |
| Default params | `scope=review_corpus`, `bates_prefix=PROD`, `fail_if_withheld=false`, `export_eml_if_missing_native=true`, `include_csv_twin=true`, `expand_family=false`, `require_qc_pass=true` |

```json
{
  "scope": "review_corpus",
  "name": "Production 1",
  "bates_prefix": "PROD",
  "fail_if_withheld": false,
  "export_eml_if_missing_native": true,
  "include_csv_twin": true,
  "expand_family": false,
  "require_qc_pass": true,
  "output_dir": null
}
```

`scope: "item_ids"` requires `item_ids: ["…"]`.

Default output root: `<matter>/exports/productions/<name_or_stamp>/`.

## Volume layout

```text
<vol>/
  DATA/
    load.dat          # Concordance-style (UTF-8 BOM required)
    load.csv          # optional twin
  NATIVES/
    <CONTROL>.<ext>
  TEXT/
    <CONTROL>.txt
  README.txt
```

Load-file paths are **Windows-style relative** (e.g. `NATIVES\PROD000001.eml`).

## DAT format (`matter_produce_v1`)

| Setting | Value |
|---|---|
| Encoding | UTF-8 **with BOM** `EF BB BF` |
| Qualifier | `þ` (U+00FE) |
| Separator | `¶` (U+00B6) |
| In-field newlines | `®` (U+00AE) |
| Datetimes | UTC only `YYYY-MM-DDTHH:MM:SSZ` |

### Fields (order)

`BEGBATES`, `ENDBATES`, `CONTROL_NUMBER`, `ITEM_ID`, `PARENT_ITEM_ID`, `FAMILY_ID`, `CUSTODIAN`, `FILE_NAME`, `FILE_EXT`, `FILE_CATEGORY`, `MIME_TYPE`, `FILE_SIZE`, `SHA256`, `DATE_SENT`, `DATE_RECEIVED`, `DATE_CREATED`, `FROM`, `TO`, `CC`, `BCC`, `SUBJECT`, `NATIVE_PATH`, `TEXT_PATH`, `HAS_REDACTED_TEXT`, `WITHHELD`, `PROD_STATUS`

**Forbidden:** notes body, privilege description / basis narrative, highlight quotes.

`FILE_EXT` / `NATIVE_PATH` / `FILE_SIZE` / `SHA256` / preferably `MIME_TYPE` are taken from the **produced** artifact (e.g. synthetic EML → `eml`, not stale `.msg`).

## Gates

| Gate | Behavior |
|---|---|
| Withhold | Skip (default) or `fail_if_withheld=true` abort; never write native/text/DAT |
| Redaction | `redaction_count > 0` → require `redacted_text_sha256`; never fall back to original |
| ICS child | Uses that item's `native_sha256` only |
| EML | Export-only packaging when native missing (not CAS identity) |
| Family expand | Default **off**; broken-family QC → track **0041** |
| **QC pass** (`require_qc_pass`, default **true**) | Requires a fresh passed `qc_runs` row for the same scope + selection fingerprint (count + sorted-id SHA-256). Missing / failed / **stale** QC → fail closed. Run `matter-qc` first. Set `require_qc_pass=false` only when deliberately bypassing. |

## Schema

Requires matter-core **schema v20** tables `production_sets` / `production_items`, and **v21** `qc_runs` when `require_qc_pass` is used.

## Audit

`produce.start` / `produce.complete` / `produce.fail` with selected / produced / skipped_withheld / skipped_other / errors.

## Resume

Checkpoint stores ordered ids, next sequence, done item ids, and counts. Completed rows are **not** renumbered on resume. Partial volumes get `status=partial` on the production set.

## Tests

```powershell
cargo test -p matter-produce
```
