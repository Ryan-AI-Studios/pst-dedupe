# matter-produce

Production export for Dedupe Desk (tracks **0040** / **0060**): package a review set as **natives + text + Concordance load file**, parameterized by **production profiles**.

> **Not legal advice.** Built-in production profiles are **technical packaging templates**
> (field maps, delimiters, Bates pad, layout, bound QC packs). Operators must validate
> output against the actual ESI protocol / meet-and-confer. Profiles do **not** certify
> court or jurisdiction compliance.

## Job

| Item | Value |
|---|---|
| Kind | `produce` (alias `production_export`) |
| Stage | `produce` |
| Default profile | `us_concordance_native_text_v1` |
| Default params | `scope=review_corpus`; packaging/Bates prefix from profile when omitted; **`bates_start` required**; `fail_if_withheld=false`; profile defaults for export EML / CSV twin / expand / `require_qc_pass` |

```json
{
  "scope": "review_corpus",
  "name": "Production 1",
  "production_profile": "us_concordance_native_text_v1",
  "bates_prefix": "PROD",
  "bates_start": 1,
  "fail_if_withheld": false,
  "export_eml_if_missing_native": true,
  "include_csv_twin": true,
  "expand_family": false,
  "require_qc_pass": true,
  "output_dir": null
}
```

`scope: "item_ids"` requires `item_ids: ["…"]`.

**Precedence:** job param > profile > engine default.

**Bates start** (`bates_start`) is **job-time only and required** — never stored in a profile (avoids multi-volume collisions). Multi-volume: set `bates_start` to the next free number (e.g. volume 2 starts at 5001). CLI: `--bates-start <n>`. Desk injects `1` for the first volume; subsequent volumes must be set explicitly.

Default output root: `<matter>/exports/productions/<name_or_stamp>/`.

## Production profiles (0060)

| Slug | Purpose |
|---|---|
| `us_concordance_native_text_v1` | **Default** — Concordance DAT, US date formats (`%m/%d/%Y` + `America/New_York`), `qc_default_v1` |
| `us_concordance_rel_alias_v1` | Same packaging; Relativity-oriented **header aliases** only |
| `us_strict_qc_concordance_v1` | Same packaging as default + `qc_strict_privilege_v1` |

Matter-local profiles: `production_profiles` table (schema **v38**); list = built-ins ∪ user. CLI: `pst-dedup production-profile list|show|upsert|delete`.

### Field map

Each entry: `{ source, header, include, date_format?, timezone? }`. Unknown sources and privilege/work-product fields fail closed at validate. Datetime sources convert **UTC → timezone → strftime** before DAT write.

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
| Datetimes | Profile field map (`date_format` + IANA `timezone`); US built-ins use `MM/DD/YYYY` in `America/New_York`. Engine default when unset: UTC ISO `YYYY-MM-DDTHH:MM:SSZ` |

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
| **QC pass** (`require_qc_pass`, default **true** via profile) | Requires a fresh passed `qc_runs` row for the same scope + **pack-aware** selection fingerprint (sorted-id SHA-256 + `#pack=<pack_id>` + count). Missing / failed / **stale** (including different pack) → fail closed. Run `matter-qc` first. Set `require_qc_pass=false` only when deliberately bypassing. |

## Schema

Requires matter-core **schema v20** tables `production_sets` / `production_items`, **v21** `qc_runs` when `require_qc_pass` is used, and **v38** `production_profiles` + `production_sets.profile_slug`.

## Audit

`produce.start` / `produce.complete` / `produce.fail` with selected / produced / skipped_withheld / skipped_other / errors, plus **profile slug**, **config hash**, **QC pack id**, and **Bates range** (`bates_start` / `bates_end`).

## Resume

Checkpoint stores ordered ids, next sequence, done item ids, and counts. Completed rows are **not** renumbered on resume. Partial volumes get `status=partial` on the production set.

## Tests

```powershell
cargo test -p matter-produce
```
