# matter-promote

Matter-level **promote-to-review** corpus (track **0025**). Builds an explicit
review set membership (`in_review`, `review_order`) for linear review (**0026**).

**Flag-only:** never deletes items or CAS blobs.

## Job kind

| | |
|---|---|
| Kind | `promote` |
| Stage | `promote` |
| Option C | Handler does **not** `create_job` |

## Params (defaults)

```json
{
  "policy": "auto",
  "review_set_name": "Review Corpus",
  "expand_families": true,
  "reset": true,
  "batch_size": 500,
  "require_dedupe": false
}
```

| Param | Default | Notes |
|---|---|---|
| `policy` | `auto` | See policies below |
| `expand_families` | `true` | Bidirectional parent↔child expand |
| `reset` | `true` | Clear prior membership for the set, then recompute |
| `require_dedupe` | `false` | If `true` and no `dedup_role` anywhere, fail |
| `expand_threads` | `false` | Reserved (0056); `true` is rejected |

## Policies

Detect “cull has run”: `EXISTS (SELECT 1 FROM items WHERE cull_status IS NOT NULL)`.

| Policy id | Selects |
|---|---|
| **`auto`** | `cull_included` if cull has run, else `unique_only` |
| **`cull_included`** | `cull_status = 'included'` |
| **`unique_only`** | `dedup_role IN ('unique','skipped')` or (`dedup_role` NULL + extracted-like). If **no** item has any `dedup_role`, all extracted-like are eligible (P0). |
| **`unique_plus_family`** | unique_only base + expand |
| **`all_extracted`** | `status IN ('extracted','partial','normalized')` |
| **`cull_included_plus_family`** | cull included + expand |

## Bidirectional family expand

When `expand_families: true` (or a `*_plus_family` policy):

1. Base set **S** from policy
2. **Down:** add direct children of S
3. **Up:** add parents of S (`parent_item_id`)
4. Repeat until fixed point (P0: ≤2 iterations, depth ≤2)
5. Expanded members get `in_review=1` even if base policy would exclude them
6. Does **not** expand threads

## Ordering (`review_order`) — single SQL, no N+1

Dense ranks **1..N** from one ordered query (temp-table join):

```sql
ORDER BY
  COALESCE(parent_item_id, id) ASC,
  CASE WHEN parent_item_id IS NULL THEN 0 ELSE 1 END ASC,
  path ASC,
  id ASC
```

Parent (or standalone) first within each family group, then children by path/id.
**Forbidden:** per-parent child queries while streaming parents.

## 0026 query contract

```sql
SELECT * FROM items
WHERE in_review = 1
  AND (review_set_id = :default_set OR :default_set IS NULL)
ORDER BY review_order ASC;
```

Default list **is** the review corpus (already reduced by promote policy).

## Schema

Requires matter-core **schema v7**: `review_sets` table + item membership columns
+ partial unique index (one `is_default=1` per matter).

## Checkpoint atomicity

Membership updates + `put_checkpoint` commit in the **same** SQLite transaction
(`Matter::apply_promote_batch_with_checkpoint`). Cancel → Paused + checkpoint;
resume continues from `cursor_index` over a frozen ordered id list.
