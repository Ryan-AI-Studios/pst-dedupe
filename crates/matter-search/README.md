# matter-search

Per-matter **Tantivy** full-text keyword search for Dedupe Desk (track **0029**).

SQLite (`matter-core`) remains **metadata-only** — **no FTS5 as primary**. Tantivy
segments live under `<matter_root>/index/`.

## Pin

| Crate | Version |
|---|---|
| **tantivy** | **0.26.x** (workspace; locks to **0.26.1**) |

**Default features** kept: `mmap`, plus optional `stopwords` / `stemmer` crates
bundled by Tantivy 0.26 defaults (available if a field selects those tokenizers).

**P0 field analyzer (actual):** schema fields use Tantivy `TEXT`, which selects
the built-in **`default` tokenizer** (simple Latin tokenization / lowercasing).
It does **not** automatically enable `en_stem` or stopword filtering unless a
custom `TextFieldIndexing` is configured — **P0 does not**. Operators should not
expect stem expansion or stopword removal. Avoid `quickwit` and other optional
features.

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
| `item_id` | `STRING \| STORED \| FAST` | **Untokenized** — exact `delete_term` + stored + fast |
| `subject` | `TEXT` | Tokenized + positions (`default` tokenizer; no stem/stop) |
| `body` | `TEXT` | CAS plain text (prefer `text_sha256`; else HTML strip) — **not STORED** |
| `path` | `TEXT` | Path / filename tokens |
| `attach_names` | `TEXT` | Concat of attachment child filenames |

## Query dialect (Tantivy `QueryParser`)

- **Boolean:** `AND` / `OR` / `NOT` / grouping with parentheses
- **Phrases:** `"quoted phrase"`
- **Default multi-term:** **AND** (`set_conjunction_by_default`)
- **Fields searched:** subject, body, path, attach_names
- **Tokenizer:** Tantivy `default` (simple tokenize + lowercase). **Stemming OFF**, **stopwords OFF** in P0 schema
- **Hit cap:** at most **`DEFAULT_FTS_FETCH_LIMIT` (50_000)** unique `item_id`s are fetched for compose / status. Larger result sets are truncated at that window (document for operators; keyset/streaming deferred).
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
| Incremental | `fts_text_sha256` stores **payload digest** = SHA-256(body_sha ∥ subject ∥ path ∥ attach_names); re-indexes when body **or** searchable metadata changes |
| Orphan purge | Items with `fts_*` set but no text / ineligible status → `delete_term` + clear bookkeeping |
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
