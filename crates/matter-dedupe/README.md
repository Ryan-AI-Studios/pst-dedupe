# matter-dedupe

Matter-level **tiered deduplication** over Normalized Items (track **0021**).

Runs as a resumable `process-runner` job (`kind = "dedupe"`). Does **not** call
`create_job` (Option C — the runner owns job rows).

## Matching policy (P0)

| Tier | Key | Notes |
|---|---|---|
| **1** | Normalized **Message-ID** | Definitive when present (`normalize_message_id`) |
| **2** | Desk **`logical_hash` v1** | When MID missing / empty |
| — | **`native_sha256`** | Custody / bit identity only — **not** a parent suppress key |
| — | CLI content-hash | **Forbidden** as Desk suppress key |

### MID vs logical conflict (Policy A)

Same MID collapses even when `logical_hash` differs. Count is recorded as
`mid_logical_conflicts` on the summary / audit. Set `use_message_id: false` for
logical-only. Policy B (logical wins on conflict) is deferred.

### Stable order (first-seen wins)

`imported_at ASC`, `path ASC`, `id ASC`.

### Eligible parents

`(role = 'parent' OR (file_category = 'email' AND role ≠ attachment))` and
`status IN ('extracted','partial','normalized')`.

## Family policy

| `family_policy` | Behavior |
|---|---|
| `suppress_children_with_parent` (default) | When parent → `duplicate`, mark direct attachments `dedup_role=duplicate`, `dedup_tier=family` |
| `parents_only` | Parents only; leave attach roles null |

### Attachment `duplicate_of` linking

Resolve target on the **canonical parent** in order:

1. Same `native_sha256`
2. Same case-folded filename + `size_bytes`
3. Else unmatched: still `duplicate` / `family`, `duplicate_of = NULL`, set
   `extra_json.family_attach_unmatched = true`

**Never** set attach `duplicate_of` to the parent **email** item id. Family graph
(`family_id`, `parent_item_id`) stays intact. **Never delete** items or CAS blobs.

## Memory posture

Canonical maps use **fixed-size `[u8; 32]` keys**:

- `logical_hash` 64-hex → decode to 32 bytes
- Message-ID → normalize then SHA-256 → 32 bytes

Parents are streamed as thin `DedupeCandidate` rows (`id`, `message_id`,
`logical_hash`, …) — **not** full `Item` structs with body text. Avoid
`HashMap<String, String>` of raw multi-MB MID strings for entire corpora.

## Transactions (checkpoint atomicity)

Each batch of role field updates and the job `put_checkpoint` for stage
`dedupe` commit in **one** SQLite transaction via
`Matter::apply_dedup_batch_with_checkpoint`. Cancel between batches →
`Paused` with committed cursor.

## Params JSON

```json
{
  "use_message_id": true,
  "use_logical_hash": true,
  "family_policy": "suppress_children_with_parent",
  "reset": true,
  "batch_size": 500
}
```

- **`reset: true` (default):** clear prior dedupe fields for the eligible set,
  then full recompute. On resume after a committed checkpoint, reset is skipped.
- **`reset: false`:** only assign where `dedup_role` IS NULL (incremental).

## Public API

```rust
use matter_dedupe::{run_dedupe, DedupeParams, DedupeOutcome};

let outcome = run_dedupe(
    &matter,
    job_id,           // runner-created
    &DedupeParams::default(),
    Some(&|| cancel_token.is_cancelled()),
    |completed| { /* progress */ },
)?;
```

## Audit

| Action | Payload |
|---|---|
| `dedupe.start` | params |
| `dedupe.complete` | unique, duplicate, skipped, mid_logical_conflicts, duration_ms |
| `dedupe.fail` | error (+ partial counts when available) |

## Tests

```powershell
cargo test -p matter-dedupe
```
