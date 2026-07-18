# matter-cull

Matter-level **flag-only data reduction** (track **0024**). Writes `cull_status`
(`included` \| `culled`) and reason codes without deleting items or CAS blobs.

## Built-in presets

| Name | Intent |
|---|---|
| `unique_only` | Cull `dedup_role=duplicate`; status gate on extracted/partial/normalized |
| `unique_plus_family` | unique_only + absolute family keep-children |
| `date_window` | unique_only + date filter template (operator fills offset-aware bounds). Desk UI omits this until bounds are set; use JSON params / user preset with `start`/`end`. |
| `noise_light` | unique_only + zero-size empty + exclude common executable MIME prefixes |

User presets live in `cull_presets` (matter-core schema v6). Built-ins are code
constants and do not require a DB row.

## Rules (`CullRules` JSON v1)

Item is **culled** if **any** enabled condition matches; **all** matching reason
codes are collected into `cull_reasons_json`.

Stable reasons: `exact_duplicate`, `date_out_of_range`, `date_missing`,
`custodian`, `path`, `file_category`, `mime`, `size`, `empty`, `status`,
`near_dup_member`, `denist`, `family_with_culled_parent`, `other`.

### Family integrity (default)

`family_policy: keep_children_with_included_parent` is **absolute**: after the
item rule pass, every **direct child** of an included parent is forced
`included` and reasons cleared — even when the child is an exact duplicate
attachment. This preserves review family integrity (four corners of the parent
document + attachments).

Other policies: `independent`, `cull_children_with_parent`.

### Date bounds (timezone-safe)

When `date.enabled` and `start` / `end` are set, each bound **must** be RFC3339
with an explicit offset or `Z` (e.g. `2023-01-01T00:00:00-05:00`). Naive strings
are rejected at job start. Comparison is in UTC; **start inclusive, end exclusive**.

Default `missing_policy` is `include` (do not drop undated mail silently).

### Near-dup

Near-dup members are **not** culled by default (`near_dup.enabled: false`).
Near-dups are not exact identity.

### DeNIST / NSRL (optional)

```json
"denist": { "enabled": false, "hash_list_path": null }
```

When enabled:

- Matches **`native_sha256` only** (64 hex, case-insensitive) against a local
  one-hash-per-line text file (`#` comments ok).
- **Missing/unreadable path → job fails** (no silent skip).
- **Legacy NSRL RDSv2 MD5/SHA-1 lists will not match.** Export **SHA-256** from
  modern NSRL RDSv3 (or equivalent).
- If the file has zero valid 64-hex lines but has 32-hex (MD5) and/or 40-hex
  (SHA-1) lines → **fail** with `denist_hash_format` (not 0-match “success”).
- Empty list after parse → **fail**.

Hash lists are operator-local paths — **never** commit NSRL bulk data to git.

## Job

| Item | Value |
|---|---|
| Kind | `cull` |
| Stage | `cull` |
| Params | `{ "preset_name": "unique_only", "reset": true, "batch_size": 500 }` or `preset_id` / inline `rules` |
| Option C | Handler does **not** `create_job` |
| Checkpoint | Same SQLite transaction as field updates |

## Promote handoff (0025)

When a matter has any non-null `cull_status`, default promote selection should
prefer `cull_status = included`. If cull has never run, fall back to unique-only
(`dedup_role`).

## API

```rust
use matter_cull::{run_cull, CullParams, JOB_KIND_CULL};

let params = CullParams {
    preset_name: Some("unique_only".into()),
    ..Default::default()
};
let outcome = run_cull(&matter, &job_id, &params, cancel, |n| { /* progress */ })?;
```
