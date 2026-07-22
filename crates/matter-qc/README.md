# matter-qc

Pre-production **QC engine** for Dedupe Desk (tracks **0041** / **0060**). Scans a produce candidate set, emits structured findings, writes a CSV report pack, and records a `qc_runs` row with a **selection + pack fingerprint** so produce can refuse stale QC (including pack changes).

## Job

| Item | Value |
|---|---|
| Kind | `qc` |
| Stage | `qc` |
| Default pack | `qc_default_v1` (legacy alias: `default_production_qc_v1`) |
| Default scope | `review_corpus` (`in_review = 1`) |

```json
{
  "scope": "review_corpus",
  "item_ids": [],
  "expand_family_for_scan": false,
  "rules": [],
  "report_dir": null,
  "profile": "qc_default_v1",
  "pack_id": null
}
```

Empty `rules` → pack severities. Per-rule severity override: `off` / `warn` / `error` (applied after pack).

`pack_id` (when set) overrides `profile` for severity resolution and fingerprinting. Produce profiles bind a pack via `qc.pack_id`.

## QC packs (0060)

| Pack id | Intent |
|---|---|
| `qc_default_v1` | Current 0041 defaults (alias `default_production_qc_v1`) |
| `qc_strict_privilege_v1` | `withheld_in_selection`, `withheld_family_member`, `broken_family_incomplete_parent` → **Error** |
| `qc_native_heavy_v1` | `missing_native` / `zero_size` → **Error**; missing text stays taxonomy-aware Warn base |

Fingerprint = SHA-256 of sorted item ids + `#pack=<pack_id>`. Different pack → produce gate miss under `require_qc_pass`.

## Default pack `qc_default_v1`

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

`matter-produce` param `require_qc_pass` (default **true** via production profile):

1. Select current produce candidates
2. Load latest `qc_runs` for matching scope
3. Require `passed` **and** fresh selection fingerprint: sorted-id SHA-256 **+ `#pack=<pack_id>`** + count + scope
4. Else fail closed: “QC required” / “QC failed” / “QC stale: selection changed…”

A QC pass under `qc_default_v1` does **not** authorize produce bound to `qc_strict_privilege_v1` (and vice versa).

Helpers:

- `matter_core::selection_fingerprint_with_pack` / `qc_run_is_fresh_for_pack`
- `matter_qc::check_qc_gate_for_pack(matter, scope, ids, pack_id)`
- Legacy `check_qc_gate` / empty-pack fingerprints are for pre-0060 rows only — Desk soft-gate and produce use pack-aware helpers

## Schema

Requires matter-core **schema v21** table `qc_runs` (current workspace schema **v38**). Pack id is stored in `qc_runs.profile`.

## Audit

`qc.start` / `qc.complete` / `qc.fail` with counts, fingerprint, **pack_id**, report path.
