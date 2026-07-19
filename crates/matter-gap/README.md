# matter-gap

Gap analysis for Dedupe Desk (track **0042**): expected-custodian roster, date-window coverage, and opposing / prior production DAT set-diff.

## Capabilities

| Pillar | What it does |
|---|---|
| **Collection gap** | Compare **expected custodians** (CSV roster) to item inventory; flag missing / unexpected |
| **Date coverage** | Optional `window_start` / `window_end`; empty window = **error**; interior **week** or **month** holes = **warn** (day buckets forbidden) |
| **Opposing DAT** | Parse Concordance DAT (0040: UTF-8 BOM, `þ`/`¶`/`®`) or simple CSV into expected docs; join to matter |

## Roster CSV

UTF-8 CSV with header:

| Column | Required | Notes |
|---|---|---|
| `custodian` | yes | Display name; matched case-insensitively after trim + whitespace collapse |
| `alias` | no | Stored in notes prefix for residual fuzzy work |
| `notes` | no | Free text |

```text
custodian,notes
Alice Smith,primary
Bob Jones,
```

**`missing_custodian` severity is always `warn`** (not error): without fuzzy alias tables, spelling variants are common.

## Opposing DAT column map

Default map matches **matter_produce_v1** field names:

`CONTROL_NUMBER`, `SHA256`, `ITEM_ID`, `CUSTODIAN`, `FILE_NAME`, `FILE_EXT`, `FILE_CATEGORY`, `MIME_TYPE`, `DATE_SENT`, `DATE_RECEIVED`, `DATE_CREATED`

Optional when present: `MESSAGE_ID`, `LOGICAL_HASH`.

Foreign DATs: pass a JSON object of `HeaderName → MappedField` where targets are **enum allowlist only** (`control_number`, `sha256`, `message_id`, …). Unknown targets → `InvalidColumnMap`. Missing core headers → `InvalidDatHeader`. Inserts use bound parameters only.

Caps (fail closed): **256 MiB**, **2_000_000** rows.

## Join key order (email-aware)

```text
1a. Message-ID (normalized; empty never matches)
1b. If email-like and no MID hit: item_id / logical_hash
2.  native SHA-256
3.  production control_number
```

Native SHA-256 alone is **not** used as the sole join for the whole set — re-serialized email natives almost never match across platforms.

## Report pack

`exports/gap/gap_<stamp>/`:

- `summary.csv`
- `missing_custodians.csv`
- `custodian_inventory.csv`
- `date_coverage.csv` (when window used)
- `opposing_summary.csv`
- `expected_not_in_matter.csv`
- `matched.csv` (thin ids)

**Subjects are omitted** by default (not stored on `gap_expected_docs`).

## Job

Kind `"gap"` via process-runner (Option C: runner creates the job). Params:

```json
{
  "kind": "collection",
  "window_start": null,
  "window_end": null,
  "bucket": "week",
  "flag_unexpected_custodian": true,
  "import_id": null,
  "matter_scope": "inventory"
}
```

Audit: `gap.roster_import`, `gap.opposing_import`, `gap.run.start`, `gap.run.complete`.

## Evidence policy

Opposing production files are **operator-local only**. Never commit client opposing DATs. Synthetic fixtures live under `fixtures/gap/`.

## Schema

Requires matter-core **schema v22** (`expected_custodians`, `expected_sources`, `gap_imports`, `gap_expected_docs`, `gap_runs`).
