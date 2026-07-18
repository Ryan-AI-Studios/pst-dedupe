# matter-search

Per-matter **Tantivy** full-text keyword search for Dedupe Desk (track **0029**).

SQLite (`matter-core`) remains **metadata-only** — **no FTS5 as primary**. Tantivy
segments live under `<matter_root>/index/`.

## Pin

| Crate | Version |
|---|---|
| **tantivy** | **0.26.x** (workspace; locks to **0.26.1**) |

**Default features** kept: `mmap`, default tokenizers (stopwords / stemmer available
via Tantivy defaults). Avoid unnecessary feature flags.

**MSRV:** tantivy 0.26 requires Rust **≥ 1.86** (project already meets this).

## On-disk layout

```text
<matter_root>/
  matter.db
  blobs/
  index/          # Tantivy directory (meta.json + segments)
```

## Schema fields

| Field | Type | Notes |
|---|---|---|
| `item_id` | `STRING \| STORED` | **Untokenized** — exact `delete_term` |
| `subject` | `TEXT` | Tokenized + positions |
| `body` | `TEXT` | CAS plain text (prefer `text_sha256`; else HTML strip) — **not STORED** |
| `path` | `TEXT` | Path / filename tokens |
| `attach_names` | `TEXT` | Concat of attachment child filenames |

## Query dialect (Tantivy `QueryParser`)

- **Boolean:** `AND` / `OR` / `NOT` / grouping with parentheses
- **Phrases:** `"quoted phrase"`
- **Default multi-term:** **AND** (`set_conjunction_by_default`)
- **Fields searched:** subject, body, path, attach_names
- **Not P0:** fuzzy, regex, dtSearch proximity, CJK segmenters (→ **0054**)

Invalid queries return `SearchError::InvalidQuery` (no panic).

Empty / missing index → `SearchError::IndexMissing` ("run Build / Update search index").

## Delete-before-add (required)

Tantivy is not SQL UPSERT. A crash after Tantivy commit but before SQLite marks
`fts_text_sha256` leaves the item unmarked; the next run would **add another
document** with the same `item_id`.

| Layer | Rule |
|---|---|
| Writer | Always `delete_term(item_id)` then `add_document` |
| Reader | HashSet de-dupe by `item_id` before returning hits |

## Windows `mmap` rebuild

Default Tantivy **mmap** maps segment files. On Windows, mappings hold hard OS
file locks. Before `reset: true` / `remove_dir_all(index/)`:

1. Drop all live `Index` / `IndexReader` / `Searcher` handles (Desk cache)
2. Join in-flight search threads
3. Then remove/recreate `index/` and open a fresh index
4. Clear SQLite `fts_*`; full re-index

API: `MatterIndex::shutdown()` and `remove_index_dir`.

## Job `fts_index`

| Item | Value |
|---|---|
| Kind / stage | `fts_index` |
| Params | `{ "reset": false, "batch_size": 100, "scope": "all_with_text", "writer_heap_bytes": 52428800 }` |
| Incremental | `text_sha256` (or html) ≠ `fts_text_sha256` or fts null |
| Commit order | Tantivy commit → SQLite `fts_*` + checkpoint **one txn** |
| Cancel | Between batches → Paused |

Audit: `fts_index.start` / `complete` / `fail`.

## Compose with FilterSpec (0028)

```text
hits = FTS(query) → unique item_ids
rows = FilterSpec restricted to those ids  (temp table join)
if include_family: expand AFTER intersect
```

`compose_keyword_filter(matter, root, keyword, filter, limit, offset)`.

## Tests

```powershell
cargo test -p matter-search
```
