# matter-search

Per-matter **Tantivy** full-text keyword search for Dedupe Desk (track **0029** +
multilingual packs **0054**).

SQLite (`matter-core`) remains **metadata-only** — **no FTS5 as primary**. Tantivy
segments live under `<matter_root>/index/`.

## Pin

| Crate | Version |
|---|---|
| **tantivy** | **0.26.x** (workspace; locks to **0.26.1**) |

**Default features** kept: `mmap`, plus optional `stopwords` / `stemmer` crates
bundled by Tantivy 0.26 defaults (available if a field selects those tokenizers).

**MSRV:** tantivy 0.26 requires Rust **≥ 1.86** (project already meets this).

## Encryption (track 0057)

When the matter is encrypted, FTS segments under `index/` are stored as chunked
AEAD blobs via `EncryptedDirectory`. **mmap is not used** — each open decrypts
file contents into process memory (`FileSlice` from `Vec`). Expect higher RAM
and slower open/commit vs plain matters; rebuild is required if switching
encryption mode. Plain matters keep the default mmap directory.

## Language packs (0054)

Offline packs only — **not** machine translation.

| Pack id | Tokenizer | Notes |
|---|---|---|
| `latin_default` | Tantivy `default` | English-friendly; closes D-0029-02 with CJK pack as alternative |
| `cjk_ngram_v1` | `cjk_hybrid_v1` | Hybrid script-boundary (see below) |

### Fingerprint (stable)

Stored on `matters.fts_lang_fingerprint` after a **Succeeded** `fts_index` only:

```text
pack={id};ver={n};ngram={min}-{max};tok={tokenizer_id};schema=fts_v1
```

Examples:

- `pack=latin_default;ver=1;ngram=0-0;tok=default;schema=fts_v1`
- `pack=cjk_ngram_v1;ver=1;ngram=2-2;tok=cjk_hybrid_v1;schema=fts_v1`

Also sets `fts_lang_built_at` (RFC3339).

### Hybrid CJK tokenizer (`cjk_hybrid_v1`)

| Script | Tokenization |
|---|---|
| Han / Hiragana / Katakana / Hangul | Character **bigrams** (`min_gram=2, max_gram=2`) with **sequential positions**; single leftover CJK char → unigram |
| Latin / Cyrillic / other non-CJK | Lowercase + simple word split; **emails kept intact** (`bob@example.com` is not split on `@` / `.`) |

### CJK query → phrase (mandatory)

Consecutive CJK characters in the user query are rewritten to Tantivy **phrase**
queries (positional adjacency) before `QueryParser`. Free AND of unigrams is
**not** used for consecutive CJK (avoids massive false positives).

Latin tokens in the same query keep Boolean / default semantics.

### Stale index hard-block

If `matters.lang_pack_id` (+ version/params) does **not** match
`fts_lang_fingerprint` of the last successful build:

- Search API returns a **hard error** — never queries a mismatched index
- Message includes stable code **`fts_lang_pack_stale`**
- Desk surfaces the error + **Rebuild FTS** CTA (not an empty success list)

Pack change clears fingerprint/built_at → forces rebuild. `fts_index` rebuilds
with the active pack tokenizer and writes the new fingerprint on **Succeeded**.

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
| `subject` | TEXT (pack tokenizer) | Tokenized + positions |
| `body` | TEXT (pack tokenizer) | CAS plain text — **not STORED** |
| `path` | TEXT (pack tokenizer) | Path / filename tokens |
| `attach_names` | TEXT (pack tokenizer) | Concat of attachment child filenames |

## Query dialect (Tantivy `QueryParser`)

- **Boolean:** `AND` / `OR` / `NOT` / grouping with parentheses
- **Phrases:** `"quoted phrase"` (CJK consecutive runs auto-quoted under CJK pack)
- **Default multi-term:** **AND** (`set_conjunction_by_default`)
- **Fields searched:** subject, body, path, attach_names
- **Hit cap:** at most **`DEFAULT_FTS_FETCH_LIMIT` (50_000)** unique `item_id`s
- **Production path:** `search_keyword_for_matter` (stale gate + pack)

Invalid queries return `SearchError::InvalidQuery` (no panic).

Empty / missing index → `SearchError::IndexMissing`.

Pack mismatch → `SearchError::LangPackStale` (`fts_lang_pack_stale`).

## Language detection (thin, optional)

`matter_core::detect_language_tag(text) -> String`:

| Rule | Result |
|---|---|
| Unicode scalar length **&lt; 50** | `und` |
| whatlang not reliable and confidence **&lt; 0.8** | `und` |
| Eng / Cmn / Jpn / Kor when confident | `en` / `zh` / `ja` / `ko` (other langs → `und` in P0) |
| Otherwise | `und` |

Detection is probabilistic; mixed emails are common. Not required for CJK FTS.

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
| Pack | Reads `matters.lang_pack_id`; registers pack tokenizer; forces full rebuild when fingerprint ≠ pack |
| Incremental | `fts_text_sha256` stores **payload digest** = SHA-256(body_sha ∥ subject ∥ path ∥ attach_names) |
| Orphan purge | Items with `fts_*` set but no text / ineligible status → `delete_term` + clear bookkeeping |
| Commit order | Tantivy commit → SQLite `fts_*` + checkpoint **one txn** |
| Succeeded | Writes `fts_lang_fingerprint` + `fts_lang_built_at` |
| Cancel | Between batches → Paused |

Audit: `fts_index.start` / `complete` / `fail` (includes pack fingerprint params).

## Compose with FilterSpec (0028)

```text
hits = FTS(query) → unique item_ids   # matter-aware search
rows = FilterSpec restricted to those ids  (temp table join)
if include_family: expand AFTER intersect
```

`compose_keyword_filter(matter, root, keyword, filter, limit, offset)`.

## Honesty / limits

- Not a translation product.
- CJK n-gram index requires positional/phrase queries for consecutive CJK (auto-applied).
- Changing pack **hard-blocks** search until FTS rebuild.
- Language tags: short/low-confidence → `und` only.
- Dictionary segmenters (jieba/lindera) residual (D-0054-*).

## Tests

```powershell
cargo test -p matter-search
```
