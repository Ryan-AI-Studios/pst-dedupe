# matter-qc

Pre-production **QC engine** for Dedupe Desk (track **0041**). Scans a produce candidate set, emits structured findings, writes a CSV report pack, and records a `qc_runs` row with a **selection fingerprint** so produce can refuse stale QC.

## Job

| Item | Value |
|---|---|
| Kind | `qc` |
| Stage | `qc` |
| Default profile | `default_production_qc_v1` |
| Default scope | `review_corpus` (`in_review = 1`) |

```json
{
  "scope": "review_corpus",
  "item_ids": [],
  "expand_family_for_scan": false,
  "rules": [],
  "report_dir": null,
  "profile": "default_production_qc_v1"
}
```

Empty `rules` → default pack. Per-rule severity override: `off` / `warn` / `error`.

## Default pack `default_production_qc_v1`

| Rule id | Default | Finding when |
|---|---|---|
| `broken_family_orphan_child` | **error** | Non-null `parent_item_id` not in candidate set |
| `broken_family_incomplete_parent` | **warn** | Selected parent has **any** non-withheld child not in set (partial family) |
| `withheld_in_selection` | **error** | Candidate is withheld |
| `withheld_family_member` | **warn** | Candidate not withheld, but parent or child is |
| `redacted_text_missing` | **error** | `redaction_count > 0` and no `redacted_text_sha256` |
| `missing_native` | **error** | No `native_sha256` and not email-like (EML-eligible) |
| `missing_text` | **dynamic** | No usable text; **error** for email/document/spreadsheet/presentation/pdf; **warn** for image/media/other/NULL |
| `pdf_needs_ocr` | **warn** | `pdf_needs_ocr = 1` |
| `zero_size` | **warn** | `size_bytes = 0` |
| `item_status_error` | **warn** | status is `error` or `partial` |
| `empty_selection` | **error** | Zero candidates |
| `only_withheld` | **error** | All candidates withheld |

`passed` = zero Error-severity findings (warnings allowed). Severity `off` skips the rule entirely.

### Broken family (normative)

```text
orphan_child :=
  parent_item_id IS NOT NULL
  AND parent_item_id NOT IN candidate_ids

incomplete_parent :=
  item ∈ candidates
  AND EXISTS non-withheld child C with parent_item_id = item.id
  AND C.id NOT IN candidates
```

| Case | Expected |
|---|---|
| Parent only, 0 of N non-withheld kids | `incomplete_parent` |
| Parent + **1 of 3** non-withheld kids | **`incomplete_parent` must fire** |
| Parent + all non-withheld kids | no incomplete |
| Parent + all non-withheld kids, one withheld unselected | no incomplete for that kid; `withheld_family_member` |
| Child without parent in set | `broken_family_orphan_child` |

## Report pack

Under `<matter>/exports/qc/qc_YYYYMMDD_HHMMSS/` (or `report_dir`):

| File | Content |
|---|---|
| `summary.csv` | metric,value (counts, passed, fingerprint, profile) |
| `findings.csv` | rule_id, severity, item_id, message |
| `README.txt` | privacy notes |

**Privacy:** item_id + short rule messages only — never subject/body/paths.

## Produce gate

`matter-produce` param `require_qc_pass` (default **true**):

1. Select current produce candidates
2. Load latest `qc_runs` for matching scope
3. Require `passed` **and** fresh selection fingerprint (sorted-id SHA-256 + count + scope)
4. Else fail closed: “QC required” / “QC failed” / “QC stale: selection changed…”

Helpers: `matter_core::selection_fingerprint`, `matter_core::qc_run_is_fresh`, `matter_qc::check_qc_gate`.

## Schema

Requires matter-core **schema v21** table `qc_runs`.

## Audit

`qc.start` / `qc.complete` / `qc.fail` with counts, fingerprint, report path.
