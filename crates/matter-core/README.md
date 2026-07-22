# matter-core

Library crate that owns the on-disk **matter** store for Dedupe Desk:

1. Matter directory layout + SQLite metadata (`matter.db`)
2. Content-addressable blob store (CAS) for **raw physical bytes**
3. Append-only audit log with integrity hash chain
4. Jobs + checkpoints for resumable work
5. Item-level error accumulator (`item_errors`)
6. **Normalized Item** model (fields introduced across schema v2–v39; current [`SCHEMA_VERSION`] is **39**) + family graph
7. Pure **logical_hash v1** helpers (length-prefixed preimage; BCC-aware)
8. Matter-level **dedupe** result columns + transactional batch helpers (0021)
9. Email **threading** header storage + result columns + batch helpers (0022)
10. **Cull** result columns + named presets + transactional batch helpers (0024)
11. **Promote** review-set membership columns + `review_sets` + batch helpers (0025)
12. **Coding** catalog + `item_codes` membership + batch apply/remove with audit (0027)
13. **Metadata filters** + `saved_searches` + paged filtered review list (0028)
14. **FTS bookkeeping** (`fts_*`) + filtered-in-ids for Tantivy compose (0029)
15. **Notes / highlights** stand-off work-product annotations (0030)
16. **Privilege** claims + withhold holds + privilege log CSV export (0031)
17. **Redaction** regions + true redacted text CAS artifact (0032)
18. **Office extract** bookkeeping (`office_*`) for OOXML text fill (0033)

Schema version: **39** (`SCHEMA_VERSION`) — prior features through optional encryption (v35 / 0057) plus **multi-user concurrent review** tables (`matter_users`, locks, batches, QC samples, `items.review_version`, `matters.multi_user_enabled`) for track **0058**, plus optional `tenant_id` / OIDC link columns (v37 / **0059**). Production profiles (v38 / **0060**). Opt-in cloud CAS config (v39 / **0061**): `storage_backend_json` + `job_backend_kind` (default local; no secrets in DB). SQLite is **metadata-only** (no FTS5 primary); Tantivy segments live under `index/` via `matter-search`. See also `matter-storage` for `BlobStore` / S3 feature `cloud-s3`.

### Multi-user / matter service (0058)

Opt-in second product mode. **Desk solo remains default** (`multi_user_enabled = 0`).

| Topic | Rule |
|---|---|
| Exclusive open | Write open acquires OS lock on `.matter.lock`; second write-open fails closed |
| OCC | `items.review_version`; service mutates require `expected_version` → conflict on stale |
| Identity | Matter-local users + Argon2id secrets + hashed sessions; no OIDC |
| Strict actor | Service sets `Matter::set_strict_actor_mode(true)`; free-form actor rejected |
| Hosting | `matter-service` crate / `pst-dedup service serve` — loopback default; local disk only |

### Optional encryption at rest (0057)

**Opt-in** — unencrypted remains the default. Pure-Rust stack (no SQLCipher / OpenSSL):

| Layer | Mechanism |
|---|---|
| Passphrase | Argon2id → KEK wraps random DEK (`matter.crypto.json`; no raw secrets in SQL) |
| SQLite | Whole-file chunked AES-256-GCM container (`matter.db` at rest); plain session under `workspace/temp/.enc-db/` while unlocked |
| CAS | Chunked AEAD (default 1 MiB); digest = plaintext SHA-256 |
| FTS | Encrypted Tantivy Directory (no mmap; decrypt into memory) |

**Honesty:** lost passphrase ⇒ **permanent data loss**; wrong passphrase fails closed; secrets are never logged; temps stay under the matter workspace; **not FIPS-validated**. Change passphrase re-wraps the DEK only (no bulk re-encrypt). CLI env: `PST_DEDUPE_MATTER_PASSPHRASE`, `PST_DEDUPE_MATTER_NEW_PASSPHRASE`, and `PST_DEDUPE_MATTER_NEW_PASSPHRASE_CONFIRM` for change.

**Windows ACL (operator):** matter encryption protects **at rest** against offline theft of the folder. It does not replace OS isolation. Prefer a user-only ACL on the matter directory (e.g. remove `BUILTIN\Users` / Everyone inheritance; grant only the reviewing account). Desk does not auto-set ACLs in P0 — treat ACL hardening as operator responsibility (best-effort residual).

### Processing profiles (0043)

Named stage presets store a versioned body map `kind → {enabled, params}`. Execution order is **engine-hardcoded** (`CANONICAL_STAGE_ORDER`); never JSON/user order.

```text
classify → office_extract → pdf_extract → ics_extract → ocr → fts_index
  → dedupe → thread → neardup → cull → promote
```

Built-ins are **cumulative** (`reset: false` on dedupe/thread/cull/neardup/fts). OCR is **off** in `standard`; use `with_ocr` to opt in. User profiles CRUD via `list/get/upsert/delete_processing_profile`. Sequential apply is job kind `profile_run` (child job rows per stage; see process-runner).

Idempotency notes for cumulative profiles: **promote** with `reset:false` re-expands membership (may add items); **cull** skips already-flagged rows (`cull_status` non-null). Neardup residual: items with `near_dup_role` already set may still be re-sketched unless a future skip is added.

### Workflows (0044)

Declarative multi-node recipes: ordered `job` | `profile_run` | `gate` nodes. Built-ins are code constants; user workflows are matter-local. `list_workflows` = **built-ins ∪ user**.

| Topic | Contract |
|---|---|
| Param binding | **AST-only** (string-leaf `${key}` placeholders); never raw JSON text replace |
| Hard gates | `require_qc_pass`, `require_has_sources` — `soft_fail` is **rejected** at validate |
| Parent linkage | Orchestrated children set `jobs.parent_job_id` to the `workflow_run` (or nested `profile_run`) parent |
| Run job | Kind `workflow_run` with `{ "workflow_id" \| "workflow_name", "run_params"? }` |

### Conversation review queries (0056)

Read-only APIs on `Matter` for day-bucket chat review (**schema v34** reused — no v35 migration; `idx_items_conversation` from 0055).

| API | Behavior |
|---|---|
| `list_conversations` | `GROUP BY conversation_id`; optional `hit_item_ids` discovery; keyset on `(last_at DESC, conversation_id ASC)` (`after_last_at` / `after_conversation_id`); `message_count` is **full** bucket; `hit_count` is intersection |
| `list_conversation_messages` | Full day-bucket stream (`WHERE conversation_id = ?`); **no** FilterSpec WHERE; keyset after cursor; total order `(sent_at IS NULL) ASC, sent_at ASC, id ASC` (nulls last); default 100 / max 500 |
| `list_conversation_messages_before` | Older keyset page (before cursor); same total order; returns chronological ASC for UI prepend |
| `list_conversation_messages_around` | Centered handoff window (default 50 before + anchor + 50 after); must include anchor |
| `conversation_hit_id_set` / `filter_ids_in_conversation` | Badge helpers only — do not define stream content |
| `list_conversation_item_ids` | All ids in bucket for bulk `apply_codes` |
| `parent_reply_snippets` | CAS text / subject truncated to 100 chars; missing → `[unavailable]` |

**Honesty:** `conversation_id` is **UTC day-bounded** (0055). Stream always loads neighbors; filters only badge hits. Bulk day-bucket coding is an explicit desk action reusing `apply_codes` (full id audit).

## Layout

Under a caller-chosen root:

```text
<matter-root>/
  matter.db                 # SQLite (WAL) or AEAD container when encrypted
  matter.crypto.json        # optional encryption header (0057)
  blobs/sha256/<aa>/<hex>   # CAS — two-hex shard prefix (ciphertext when encrypted)
  .cache/blobs/             # optional LRU for cloud CAS gets (0061)
  index/                    # Tantivy FTS segments (encrypted Directory when encrypted)
  exports/                  # productions/ (0040), reports/, privilege logs, …
  logs/                     # optional file logs
  workspace/temp/           # extractor spill (cleaned on open/create; .enc-db preserved)
```

### Workspace temp

Evidence materialization (e.g. CAS → temp PST for extract) must use
`workspace/temp/` under the matter root — **never** OS `%TEMP%` /
`std::env::temp_dir()`.

| API | Behavior |
|---|---|
| `WORKSPACE_DIR` / `WORKSPACE_TEMP_DIR` | Layout constants |
| `Matter::workspace_temp_dir()` | Path to `workspace/temp/` |
| `Matter::cleanup_workspace_temp()` | Recursive delete of **contents** (keeps dir) |
| `Matter::create` / `Matter::open` | Ensure layout + call cleanup |

Crash residue cannot accumulate across sessions.

## CAS contract

| Decision | Choice |
|---|---|
| Algorithm | SHA-256, lowercase hex digest |
| Input | **Raw physical bytes only** (never normalized/logical content) |
| Path | `blobs/sha256/<aa>/<fullhex>` where `<aa>` is the first two hex chars |
| Collision | Existing path with different content → hard error (no overwrite) |
| Logical preimage | **Never** stored in CAS as “native”; only digests of body text may be stored as ordinary CAS blobs referenced by `text_sha256` / `html_sha256` |

### Streaming put

| API | Behavior |
|---|---|
| `Cas::put_bytes(&[u8])` | Buffer-in-memory put (small objects) |
| `Cas::put_reader(&mut impl Read)` | Hash while writing temp under `blobs/`, atomic rename; 64 KiB buffer |
| `Matter::put_reader` | Convenience wrapper |
| `Cas::open_read` | Streaming get (`File` handle) |

Use `put_reader` for multi-GB attachments so callers never hold a full payload
`Vec<u8>`. Same digest path ⇒ success (idempotent).

### Cloud BlobStore (0061) + encryption

- Desk default continues to use local `Cas` only.
- Opt-in cloud backends live in `matter-storage` (`BlobStore` trait). Config is
  stored on `matters.storage_backend_json` (no credentials). **Config alone does not
  activate cloud** — on `Matter::open`, kind S3/Azure calls `open_blob_store` and
  routes CAS convenience APIs through the remote store. Open **fails closed** without
  feature `cloud-s3` / on open errors (no silent local fallback).
- When `encryption_enabled`: **plaintext** is hashed for the CAS digest; **ciphertext**
  is stored via **`put_at_digest(plaintext_digest, ciphertext)`**. Plaintext puts use
  **`put_stream`**. Do not put plaintext case content to S3 without client-side AEAD when encryption is on.
- Cloud gets use `CachedBlobStore` under matter `.cache/` (when wrap_cache is on) to cut egress.
- `set_storage_backend_config` binds `matter_id` to this matter and aligns `tenant_id`
  with `matters.tenant_id` (rejects conflicts).

## Schema v2 — Normalized Item (P0)

Extends v1 `items` with nullable columns (safe `ALTER TABLE … ADD COLUMN` migration from v1).

| Field | Notes |
|---|---|
| *(v1 columns)* | id, matter_id, source_id, family_id, path, native_sha256, logical_hash, message_id, status, size_bytes, timestamps |
| `role` | `standalone` \| `parent` \| `attachment` (constants in `item_role`) |
| `parent_item_id` | Denorm parent link; **app-enforced** (no ALTER FK) |
| `mime_type` | Best-effort IANA type |
| `file_category` | `taxonomy_v1` closed set: `email`, `document`, `spreadsheet`, `presentation`, `pdf`, `image`, … — **not** bare `attachment` (`role=attachment` is separate; see file-category / taxonomy_v1) |
| `custodian` | Nullable |
| `subject` / `title` | Email subject vs non-email title |
| `from_addr` | Single from |
| `to_addrs_json` / `cc_addrs_json` / `bcc_addrs_json` | JSON string arrays |
| `author` | Non-email author |
| `sent_at` / `received_at` | RFC3339 UTC preferred |
| `attachment_count` | Direct children count |
| `text_sha256` / `html_sha256` | CAS digests of body text (not inline SQLite TEXT for large bodies) |
| `logical_hash_version` | INTEGER NOT NULL DEFAULT 0; set when hash computed |
| `extra_json` | Extractor escape hatch |

**Indexes (v2):** `idx_items_logical_hash`, `idx_items_message_id` (plus v1 source/family/native indexes).

## Schema v3 — Matter dedupe results (0021)

Nullable columns on `items` (does **not** overload `status`):

| Field | Notes |
|---|---|
| `dedup_role` | `unique` \| `duplicate` \| `skipped` \| NULL (not run) — constants in `item_dedup_role` |
| `duplicate_of_item_id` | Canonical unique item id when duplicate |
| `dedup_tier` | `message_id` \| `logical_hash` \| `family` \| `none` — constants in `item_dedup_tier` |
| `dedup_group_id` | Optional group key (often canonical id) |
| `deduped_at` | RFC3339 when last assigned |
| `dedup_job_id` | Last job that wrote the result |

**Indexes (v3):** `idx_items_dedup_role`, `idx_items_duplicate_of`, `idx_items_dedup_group`.

### Dedupe helpers

| API | Purpose |
|---|---|
| `list_email_parents_for_dedupe` / `_range` | Thin ordered candidates (no body text) |
| `count_email_parents_for_dedupe` | Eligible parent count |
| `count_by_dedup_role` | Aggregate unique/duplicate/skipped/null |
| `clear_dedupe_fields` | Reset columns (transactional) |
| `with_transaction` | `BEGIN IMMEDIATE` helper |
| `apply_dedup_batch_with_checkpoint` | **N role updates + checkpoint in one commit** (DoD-5) |

Engine: `crates/matter-dedupe`. Never delete items/blobs; never use CLI content-hash as suppress key.

## Case overview (0038)

Read-only rollups for the desk Overview panel and for **0039** exportable reports.
Do **not** re-implement these queries in 0039 — serialize `CaseOverview` instead.

| API | Purpose |
|---|---|
| `load_case_overview(root, &OverviewOptions)` | Concurrent fan-out: multiple short-lived `Matter::open_for_read` + `std::thread` for independent GROUP BYs, then join into `CaseOverview` |
| `load_case_overview_on(&matter, &opts)` | Sequential path on one open handle (tests / small matters) |
| `Matter::overview_*` | Slice helpers (totals, status, category, custodian, cull, review, privilege, OCR, errors, jobs) |

### Locked metric rules

| Metric | Rule |
|---|---|
| **Top-level size** | `SUM(COALESCE(size_bytes,0))` where `role IS NULL OR role != 'attachment'` — **never** unlabeled `SUM` over all rows |
| **Top-level items** | Count same role filter (not `COUNT(DISTINCT family_id)` — forbidden) |
| **families_total** | Optional cheap `COUNT(*) WHERE role = 'parent'` only |
| **Review progress** | `in_review` (default set) + `reviewed_count` (≥1 `item_codes` row) + `unreviewed_count`; invariant reviewed + unreviewed = in_review |
| **Errors** | Matter-scoped total + top-N by `code` (default 15); join item/source/job to matter |
| **Top-N defaults** | categories 25, custodians 25, error codes 15, recent jobs 5 |
| **Privacy** | Counts and labels only — no subject/body |
| **Audit** | None on view (read-only) |

Empty labels are returned as `""`; UI maps category → `(uncategorized)`, custodian → `(none)`.

## Matter report export (0039)

Exportable progress/metrics CSV pack built **only** from `CaseOverview` + `list_jobs`
(no re-implemented rollup SQL). Desk: Overview → **Export matter report…**.

| API | Purpose |
|---|---|
| `export_matter_report(root, params)` / `Matter::export_matter_report` | Write pack under `params.output_dir` (must not already exist) |
| `default_matter_report_dir(root)` | `exports/reports/matter_report_YYYYMMDD_HHMMSS/` |
| `scrub_error_summary` | Redact paths/`file://`/filenames from job errors before CSV write |
| `rfc3339_to_excel_utc` | Dual datetime helper for Excel-friendly columns |
| `MATTER_REPORT_FORMAT_VERSION` | `matter_report_v1` |

### Pack files

| File | Notes |
|---|---|
| `summary.csv` | Two-column `metric,value` — KPIs, dual `generated_at` / `generated_at_excel`, job state counts |
| `by_file_category.csv` / `by_custodian.csv` / `by_status.csv` | `label,count` (+ `(other),N`); empty → header + `(none),0` |
| `errors_by_code.csv` | `code,count`; zero errors still have sentinel (never 0-byte) |
| `jobs.csv` | All jobs newest-first; dual times; `error_summary_safe` (scrubbed) |
| `README.txt` | Privacy + datetime note |

**Privacy:** counts, labels, job metadata only — no subjects, bodies, or privilege descriptions.
**Overwrite:** fail closed if target dir exists. **PDF:** deferred (**D-0039-01**); `include_pdf` ignored, `pdf_written=false`.
**Audit:** `report.export.complete` on success; `report.export.fail` on failure.

## Schema v4 — Email threading (0022)

Nullable columns on `items` (does **not** overload `status` or `dedup_*`):

| Field | Kind | Notes |
|---|---|---|
| `in_reply_to` | **Header storage** | Normalized In-Reply-To Message-ID (single; empty → NULL) |
| `references_json` | **Header storage** | JSON array of normalized Message-IDs from References (order preserved) |
| `conversation_topic` | **Header storage** | Optional Outlook/topic string as extracted |
| `conversation_index_hex` | **Header storage** | Canonical lowercase hex (MAPI bytes or Base64 Thread-Index) |
| `thread_id` | **Result** | Stable conversation group id |
| `thread_root_item_id` | **Result** | Earliest stable-order member chosen as root |
| `thread_method` | **Result** | `headers` \| `subject` \| `conversation_index` \| `singleton` \| `none` — constants in `item_thread_method` |
| `threaded_at` | **Result** | RFC3339 when last assigned |
| `thread_job_id` | **Result** | Last job that wrote the result |

**Indexes (v4):** `idx_items_thread_id`, `idx_items_in_reply_to`.

Header storage is written by extractors (`extract-pst`) and is **not** cleared by the thread job. Result columns are assigned by `matter-thread` and are what `clear_thread_fields` resets.

### Thread helpers

| API | Purpose |
|---|---|
| `list_email_parents_for_thread` / `_range` | Thin ordered candidates (`ThreadCandidate` — no body text) |
| `count_email_parents_for_thread` | Eligible parent count |
| `clear_thread_fields(include_attachments)` | Reset **result** columns only (`thread_id`, `thread_root_item_id`, `thread_method`, `threaded_at`, `thread_job_id`); leaves header storage intact |
| `apply_thread_batch_with_checkpoint` | **N result updates + checkpoint in one commit** (same DoD-5 pattern as dedupe) |

Header parse helpers live in `thread_headers` (re-exported from the crate root): `parse_in_reply_to`, `parse_references_header`, `references_to_json` / `parse_references_json`, `normalize_conversation_index_to_hex`, `unfold_header_value`.

Engine: `crates/matter-thread`. Never delete items/blobs; never mutate source PST.

## Schema v5 — Near-duplicate detection (0023)

Nullable columns on `items` (does **not** overload `dedup_*` or `thread_*`):

| Field | Kind | Notes |
|---|---|---|
| `near_dup_group_id` | **Result** | Stable group id (`SHA-256` of `near:v1\n{pivot_item_id}`) |
| `near_dup_role` | **Result** | `pivot` \| `member` \| `unique` \| `skipped` — constants in `item_near_dup_role` |
| `near_dup_similarity` | **Result** | REAL 0.0–1.0 vs group pivot (`1.0` for pivot); NULL if unique/skipped |
| `near_dup_pivot_item_id` | **Result** | Pivot item id (self if pivot) |
| `near_dup_method` | **Result** | Algorithm tag, e.g. `minhash_shingle_v1` |
| `near_duped_at` | **Result** | RFC3339 when last assigned |
| `near_dup_job_id` | **Result** | Last job that wrote the result |

**Indexes (v5):** `idx_items_near_dup_group`, `idx_items_near_dup_role`.

Near-dup results are flag-only (never suppress as exact). Engine: `crates/matter-neardup`.

### Near-dup helpers

| API | Purpose |
|---|---|
| `list_neardup_candidates` / `_range` | Thin ordered candidates (`NearDupCandidate` — id, text_sha256, dedup_role, order keys) |
| `count_neardup_candidates` | Eligible count |
| `clear_near_dup_fields` | Reset near-dup result columns (transactional) |
| `apply_near_dup_batch_with_checkpoint` | **N result updates + checkpoint in one commit** |

## Schema v6 — Cull / data reduction (0024)

Nullable columns on `items` (flag-only; never deletes items or CAS blobs):

| Field | Kind | Notes |
|---|---|---|
| `cull_status` | **Result** | `included` \| `culled` \| NULL — constants in `item_cull_status` |
| `cull_reasons_json` | **Result** | JSON array of reason codes |
| `cull_preset_id` | **Result** | Preset id that last wrote this row |
| `cull_preset_name` | **Result** | Denormalized preset name |
| `culled_at` | **Result** | RFC3339 when last assigned |
| `cull_job_id` | **Result** | Last job that wrote the result |

**Indexes (v6):** `idx_items_cull_status`, `idx_items_cull_preset`.

Table **`cull_presets`**: matter-scoped named rule sets (`list` / `get` / `upsert` / `delete`).
Deleting a preset does **not** clear item cull fields.

### Cull helpers

| API | Purpose |
|---|---|
| `list_cull_candidates` / `_range` | Thin ordered candidates (`CullCandidate`) |
| `count_cull_candidates` | Candidate count |
| `clear_cull_fields(process_attachments)` | Reset cull result columns on the eligible set only (same attachment filter as list; transactional) |
| `apply_cull_batch_with_checkpoint` | **N result updates + checkpoint in one commit** |
| `list_cull_presets` / `get_cull_preset` / `upsert_cull_preset` / `delete_cull_preset` | Preset CRUD |

## Schema v7 — Promote / review sets (0025)

| Column / table | Meaning |
|---|---|
| `review_sets` | Named review sets (`is_default`, policy snapshot, `item_count`) |
| `in_review` | 0/1 membership (NULL = never promoted) |
| `review_set_id` / `review_order` | Set membership + dense linear order for 0026 |
| `promoted_at` / `promote_job_id` / `promote_policy` | Provenance |

**Partial unique index:** `idx_review_sets_one_default ON review_sets(matter_id) WHERE is_default = 1`.

| API | Notes |
|---|---|
| `ensure_default_review_set` | Create/load default set (name default: `Review Corpus`) |
| `get_review_set` / `list_review_sets` / `update_review_set_snapshot` | Meta |
| `clear_review_membership_for_set` | Flag-only demote for recompute |
| `list_promote_candidates` | Thin rows for policy selection |
| `list_promote_ordered_membership` | **Single-query** family compound order (no N+1) |
| `list_direct_children_ids` / `list_parent_ids_of` | Bidirectional expand helpers |
| `apply_promote_batch_with_checkpoint` | **N membership updates + checkpoint in one commit** |
| `cull_has_run` / `any_dedup_role_present` | Policy resolution helpers |
| `count_in_review` / `list_review_thin` | Thin review corpus list for desk **0026** (ordered by `review_order`) |
| `get_default_review_set_id` | Default set id helper for list filter |
| `count_items_filtered` / `list_items_filtered_thin` | Metadata [`FilterSpec`] → parameterized SQL (**0028**) |
| `list_saved_searches` / `get_saved_search` / `upsert_saved_search` / `delete_saved_search` | Named saved filters (live re-run) |

`ReviewListRow` is a thin projection (no body text / participant JSON). When
`set_id` is `None`, list/count use the default review set if present, else all
`in_review = 1`.

Engine: `crates/matter-cull`. **0025 promote** should prefer `cull_status=included` when any cull has run; else unique-only.

## Schema v9 — Filters + saved searches (0028)

Structured **metadata** filters over the Review Corpus (or entire matter). Body
keyword / FTS is **0029** (Tantivy) — not SQLite FTS5 and not this track.

### `FilterSpec` (JSON)

```json
{
  "version": 1,
  "scope": "review_corpus",
  "include_family": false,
  "conditions": [
    { "field": "custodian", "op": "eq", "value": "alice@example.com" },
    { "field": "code", "op": "any_of", "values": ["responsive"] },
    { "field": "sent_at", "op": "between",
      "start": "2023-01-01T00:00:00-05:00",
      "end": "2024-01-01T00:00:00-05:00" }
  ]
}
```

| Rule | Behavior |
|---|---|
| AND only | Flat `conditions` list (no nested OR builder in P0) |
| Parameterized SQL | User strings → `?` binds only (`filter::compile_filter`) |
| Date bounds | RFC3339 **with offset or Z** required; start inclusive, end exclusive for `between`. Compared as **UTC epoch milliseconds** via `desk_utc_epoch_ms` (subseconds preserved; offset-bearing stored TEXT normalized). |
| `scope=review_corpus` | `in_review = 1` + default review set (same as `list_review_thin`) |
| `scope=entire_matter` | Status in `extracted` / `partial` / `normalized` |
| `include_family` | Conditions apply **only** in a `hits` CTE; outer SELECT is membership-by-family (parent + direct children / `family_id`). Outer still requires scope (e.g. still `in_review = 1`). |
| Sort | `(review_order IS NULL), review_order, imported_at, path, id` (emulates NULLS LAST; SQLite ASC puts NULLs first by default) |
| Index | Partial `idx_items_review_list_order ON items(review_set_id, review_order, imported_at, path, id) WHERE in_review = 1` for deep OFFSET |

**Family SQL shape (conceptual):**

```sql
WITH hits AS (
  SELECT i.id, i.family_id, COALESCE(i.parent_item_id, i.id) AS family_root
  FROM items i
  WHERE <scope> AND <conditions>   -- predicates only here
)
SELECT DISTINCT thin columns
FROM items out
WHERE <scope_on_out>
  AND (
    out.family_id IN (SELECT family_id FROM hits WHERE family_id IS NOT NULL)
    OR out.id IN (SELECT family_root FROM hits)
    OR out.parent_item_id IN (SELECT family_root FROM hits)
  )
ORDER BY ... LIMIT ? OFFSET ?;
```

### `saved_searches`

| Column | Notes |
|---|---|
| `id` / `matter_id` / `name` | UNIQUE `(matter_id, name)` |
| `scope` | Denormalized from FilterSpec |
| `filter_json` | Serialized `FilterSpec` — re-runs against **live** item state |
| `created_at` / `updated_at` / `created_by` | RFC3339 |

Audit: `search.save` / `search.delete` only (not every Apply).

Module: `matter_core::filter` (`FilterSpec`, `compile_filter`, presets).

### Annotation filter fields (0030)

| Field | Op | Meaning |
|---|---|---|
| `has_notes` | `eq` true/false | `items.note_count > 0` |
| `has_highlights` | `eq` true/false | `items.highlight_count > 0` |
| `note_text` | `contains` | EXISTS note whose body matches bound `LIKE` (case-folded) |

`FILTER_SPEC_VERSION` remains **1** (backward compatible). Presets: `FilterSpec::preset_has_notes()`, `preset_has_highlights()`, `preset_has_redactions()`, `preset_redacted_text_stale()`, `preset_withheld()`, `preset_privilege_log_incomplete()`.

### Redaction filter fields (0032)

| Field | Op | Meaning |
|---|---|---|
| `has_redactions` | `eq` true/false | `items.redaction_count > 0` |
| `redacted_text_stale` | `eq` true/false | count>0 and artifact missing, or source digest ≠ `text_sha256` when set, else ≠ `html_sha256` when text is NULL |

### Privilege filter fields (0031)

| Field | Op | Meaning |
|---|---|---|
| `privilege_withhold` | `eq` true/false | Production hold (`items.privilege_withhold` / `item_privilege.withhold`) |
| `privilege_status` | `any_of` | Status in list (`asserted`, `under_review`, `cleared`, `partial_redaction`) |
| `privilege_log_ready` | `eq` true | `include_on_log=1` AND `trim(description) != ''`; `eq` false → include_on_log blank description |

## Schema v13 — Text redaction + true redacted CAS (0032)

Stand-off **redaction** regions (black paint in Review) are **separate** from yellow
`item_highlights`. Original `text_sha256` / native CAS is **never** rewritten.
Produce-safe output is a **new** CAS blob of redacted UTF-8 text.

### Tables / columns

| Table / column | Purpose |
|---|---|
| `item_redactions` | UTF-8 **char** ranges + quote / prefix / suffix / `body_digest` + `reason` / `label` / `status` |
| `items.redaction_count` | Denormalized region count |
| `items.redacted_text_sha256` | CAS digest of last successful redacted text (NULL when absent/stale) |
| `items.redacted_text_at` | RFC3339 timestamp of last regenerate |
| `items.redacted_source_digest` | Display body digest the artifact was built from |

Indexes: `(item_id)`; `(matter_id, status)`; `(matter_id, reason)`.

### Reasons / status

| Field | Values |
|---|---|
| `reason` | `privilege` \| `pii` \| `confidential` \| `other` |
| `status` | `active` \| `stale` |
| Produce token | fixed **`[REDACTED]`** (P0 lock; stamp `label` is metadata only) |

### True redact algorithm (**mandatory merge**)

```text
build_redacted_text(display_body, ranges):
  1. Collect active [start, end) char intervals
  2. MERGE (union) overlapping/adjacent intervals  — before any mutation
  3. Replace each merged span once with [REDACTED]
```

Unmerged multi-pass replace is **forbidden** (UTF-8 panic / wrong indices). Output
must not contain any redacted `exact_quote` as a contiguous substring.

### API

| Method | Behavior |
|---|---|
| `list_redactions` / `create_redaction` / `delete_redaction` | CRUD; create validates quote==slice; create/delete **NULL** artifact pointer |
| `resolve_redactions` | In-memory status + optional persist `stale` (whitespace re-resolve like highlights) |
| `build_redacted_text` / `merge_redaction_intervals` | Pure builders |
| `regenerate_redacted_text` | Resolve → active only → merge → CAS put → set bookkeeping; empty active clears pointer |
| `invalidate_redacted_artifact` | Explicit NULL of `redacted_*` columns |

**Body digest change:** `update_item` when **`text_sha256` or `html_sha256`**
changes **NULLs** `redacted_text_sha256` / `at` / `source_digest` (defense-in-depth
for **0040**). Regenerate prefers full plain-text CAS when `text_sha256` is set
(truncated Review display cannot poison the produce artifact); HTML-only items
bookkeep `redacted_source_digest` as `html_sha256` when present.

**Privilege hook:** `reason=privilege` → ensure/upsert claim with
`status=partial_redaction`, `withhold=1`, `include_on_log=1`.

Audit: `redaction.create`, `redaction.delete`, `redaction.regenerate`.

### **Production contract for 0040** — normative

```text
if redaction_count > 0:
  if redacted_text_sha256 IS NULL:
    fail closed or force regenerate — do NOT use original text_sha256
  else:
    produce path MUST use redacted_text CAS
  MUST NOT emit original text_sha256 body as the produced text
if item_is_withheld and no redacted artifact intended:
  skip/fail natives per 0031
if withhold=1 AND redacted_text present:
  0040 may offer "produce redacted text only" (no full native)
```

## Schema v12 — Privilege claims + withhold + log export (0031)

Structured privilege workflow on top of the seed **Privilege** code (0027). Soft-clear only (no hard-delete of claim rows by default).

### Tables

| Table | Purpose |
|---|---|
| `item_privilege` | 1:1 claim: `basis`, `description`, `status`, `withhold`, `include_on_log`, asserted/updated metadata |
| `privilege_protocol` | Matter stub: `log_format`, `fre_502d_note`, `fre_502e_note`, `description_required` (informational — **not** a court order) |

Denormalized on `items`: `privilege_withhold` INTEGER NOT NULL DEFAULT 0 (maintained with privilege writes).

### Basis vocabulary

| Key | Log / UI label |
|---|---|
| `attorney_client` | Attorney-Client Privilege |
| `work_product` | Work Product |
| `attorney_client_work_product` | Attorney-Client and Work Product |
| `common_interest` | Common Interest |
| `other` | Other (see description) |

Default on ensure (Privilege code apply / Assert): `status=asserted`, `withhold=1`, `include_on_log=1`, `basis=attorney_client`, empty description OK.

Soft-clear (Privilege code remove): `status=cleared`, `withhold=0`, `include_on_log=0`, **retain description** for internal audit / re-open.

### **Withhold contract for production (0040)** — normative

```text
item_is_withheld(item) := EXISTS item_privilege WHERE item_id AND withhold = 1
```

| Rule | Requirement |
|---|---|
| **0040 gate** | Production natives / load file **must** skip or fail-closed on `item_is_withheld` / `list_withheld_item_ids` |
| **Soft-clear description** | Retained `item_privilege.description` after `status=cleared` is **internal audit only**. Privilege log CSV **never** includes cleared rows. Production load-file / natives metadata **must not** emit `item_privilege.description` (or basis narrative) for cleared rows, and should default-exclude privilege description fields entirely |
| Override | Operator may set `withhold=0` while still asserted (rare; audited) |

### Privilege log CSV columns (standard P0)

`ControlNumber`, `ParentControlNumber`, `FamilyId`, `Custodian`, `DocDate`, `From`, `To`, `Cc`, `Bcc`, `Subject`, `FileName`, `FileType`, `PrivilegeType`, `Description`, `Status`, `Withhold`, `HasPrivilegeCode`, `MatterId`, `ExportedAt`

Eligibility: `include_on_log=1` AND status ∈ `asserted` / `under_review` / `partial_redaction`. Blank descriptions export with warning count (not hard-fail). **Attachment inheritance:** empty From/To/Cc/Bcc/Subject/DocDate filled from parent email when `parent_item_id` set; FileName remains child basename. Notes body is **never** auto-copied into Description.

API: `ensure_item_privilege`, `upsert_item_privilege`, `clear_item_privilege`, `get_item_privilege` / `list_item_privilege`, `get_privilege_protocol` / `upsert_privilege_protocol`, `item_is_withheld` / `list_withheld_item_ids`, `family_privilege_consistency`, `export_privilege_log`.

Audit: `privilege.upsert`, `privilege.clear`, `privilege.protocol_upsert`, `privilege.log_export`. Coding apply/remove Privilege hooks ensure/soft-clear in the same transaction with separate privilege audit events (full sorted item ids).

## Schema v11 — Notes / highlights (0030)

Stand-off **work-product** annotations. Never rewrite CAS body text. Notes are
strategy-sensitive — matter-local; production export (**0040**) is opt-in later.

### Tables

| Table | Purpose |
|---|---|
| `item_notes` | Document or passage notes (`highlight_id` nullable); hard delete OK |
| `item_highlights` | UTF-8 **char** ranges + `exact_quote` / prefix / suffix / `body_digest` |

Denormalized on `items`: `note_count`, `highlight_count` (maintained in the same txn as mutations).

### Limits

| Limit | Value |
|---|---|
| Note body | max **64 KiB** UTF-8 (`NOTE_BODY_MAX_BYTES`) |
| Highlight quote | max **4 KiB** |
| Default color | `#FFF59D` (yellow) |
| Status | `active` \| `stale` |

### Anchoring (§3.5 / §3.5.1)

| Field | Role |
|---|---|
| `start_utf8` / `end_utf8` | Fast paint when `body_digest` matches (char indices, end exclusive) |
| `exact_quote` | Stored **raw**; re-matched with whitespace collapse when digest changes |
| `prefix` / `suffix` | ~40 chars context for disambiguation |
| `body_digest` | Prefer item `text_sha256`; else `display_body_digest(display_body)` |

**Re-resolve:** collapse Unicode whitespace runs to a single ASCII space on quote,
prefix/suffix, and body; trim quote ends; find on normalized body; map hit back to
**raw** char range for paint. Prefer offsets when digest matches. True missing quote → `stale`.

### API

| Method | Behavior |
|---|---|
| `list_notes` / `upsert_note` / `delete_note` | CRUD; empty/oversize rejected |
| `list_highlights` / `create_highlight` / `delete_highlight` | Create validates quote==slice; delete **unlinks** notes (`highlight_id` NULL) |
| `resolve_highlights` | In-memory status + optional persist `stale` |

Audit (full body / range snapshots): `note.upsert`, `note.delete` (**includes `highlight_id` when passage-linked**), `highlight.create`, `highlight.delete`.

Helpers: `resolve_highlight_against_body`, `re_resolve_whitespace_normalized`, `utf8_char_slice`, `collapse_whitespace`.

## Schema v8 — Coding / tags (0027)

Matter-scoped code catalog + item membership. Membership only — never deletes
items/CAS. All writes go through `apply_codes` (single-group rules + audit).

| Table | Purpose |
|---|---|
| `code_definitions` | Catalog: `key`, `label`, `group_key`, `cardinality` (`single`\|`multi`), `color`, `sort_order`, `is_active` |
| `item_codes` | Membership PK `(item_id, code_id)` + `set_at` / `set_by` |

**Unique:** `(matter_id, key)` on definitions. Indexes: `(matter_id, group_key, sort_order)`, `item_codes(item_id)`, `item_codes(code_id)`.

### Seed catalog (idempotent)

| key | label | group_key | cardinality |
|---|---|---|---|
| `responsive` | Responsive | responsiveness | single |
| `not_responsive` | Not Responsive | responsiveness | single |
| `needs_second_look` | Needs Second Look | responsiveness | single |
| `privilege` | Privilege | privilege | multi |
| `hot` | Hot / Key | issues | multi |
| `confidential` | Confidential | issues | multi |

`seed_default_codes` runs on `Matter::create` / `Matter::open` (insert-if-missing by key).

### API

| API | Notes |
|---|---|
| `seed_default_codes` | Idempotent seed |
| `list_code_definitions` | All defs (active + inactive), ordered |
| `upsert_code_definition` | Insert (label→slug key) or update label/group/active |
| `get_code_definition` | By id |
| `list_item_codes(item_ids)` | Batch map; includes inactive defs with historical membership |
| `apply_codes(ApplyCodesInput)` | **Add and/or remove** in one `BEGIN IMMEDIATE` txn |

**`ApplyCodesInput`:** `{ item_ids, add_code_ids, remove_code_ids, propagate_family, actor }`

| Rule | Behavior |
|---|---|
| Single-group add | Adding a `cardinality=single` code removes other codes in the same `group_key` on that item first |
| Conflicting single-group batch | Two or more `cardinality=single` codes with the same `group_key` in one `add_code_ids` are **rejected** (no silent pick) |
| Multi-group | `hot` + `confidential` both remain |
| `propagate_family` (default **false**) | Expand each selection to **parent + all direct children** (+ same non-null `family_id`); **not** near-dup or full thread |
| Audit | `coding.apply` with **complete** sorted `item_ids` of final targets (never hash/sample), plus `add`, `remove`, `propagate_family`, `selected_count`, `target_count` |
| Failed batch | No partial membership commit |

**Note:** The Privilege **code** (0027) is membership only; full claim fields, withhold holds, and privilege log CSV export ship in **0031** (schema v12 — see above).

### Address storage (JSON decision)

P0 keeps `to_addrs_json` / `cc_addrs_json` / `bcc_addrs_json` on `items` as JSON arrays.

| Concern | Decision |
|---|---|
| Ingest / extract write path | JSON arrays match extractor output |
| Free-text / fielded participant search | **Tantivy (0029)** is plan-of-record — not SQLite JSON1 |
| Comms graphs (0038, 0047) | Schema **v26** has relational `item_participants` + `people` / `people_edges` / `people_timeline` (built by `people_graph` / matter-people). JSON address columns (`from_addr`, `to_addrs_json`, `cc_addrs_json`, `bcc_addrs_json`) remain the extract source of truth for Pass 1 |

### Migration notes (v1 → v2)

- All new columns are nullable (or `logical_hash_version` DEFAULT 0).
- **0016 inventory rows** remain valid: `path` + `native_sha256` + status `discovered`/`expanded`/`error`; extended fields null; no re-ingest required.
- **NULL `role` ≡ standalone** for pre-v2 inventory until an extractor classifies the row. New inserts default `role=standalone`.
- `parent_item_id` is plain TEXT (no SQLite FK); `Matter` APIs reject missing parents and cross-matter family/parent links.
- Parent/child **family cohesion**: when `parent_item_id` is set, parent and child must share the same `family_id` (`insert_item`, `update_item`, `set_item_family_role`). If the child omits `family_id` but the parent has one, the child inherits it; a parent link with no family on either side is rejected (`Error::FamilyCohesion`).
- Parent `attachment_count` is recomputed whenever a child's `parent_item_id` is set, changed, or cleared (`insert_item`, `update_item`, `set_item_family_role`).
- Each migration step runs in a single transaction (batch + `schema_meta` version bump).
- `matters.schema_version` is re-synced on every `migrate()` (idempotent).

## Family graph

| API | Purpose |
|---|---|
| `insert_family(kind)` | Create family; empty kind → `email_attachments` |
| `get_family` / `list_family_members` | Read |
| `set_item_family_role(item, family, role, parent)` | Link parent/child + roles; recomputes old/new parent `attachment_count` |
| `list_attachments(parent_id)` / `get_parent(child_id)` | Walk graph |

**Semantics:** Parent `role=parent`; children `role=attachment` + `parent_item_id`; standalone items `family_id` null, `role=standalone` (or NULL on migrated inventory — treat as standalone). All members share `matter_id`. Parent and children share the same `family_id` (enforced). Light audit on `family.create` only (not per-field item updates).

## Status vocabulary

Constants in `item_status`:

| Status | Meaning |
|---|---|
| `discovered` / `expanded` | 0016 inventory only |
| `error` | Failed processing unit |
| `normalized` | Fields + logical_hash written without full extractor pipeline |
| `extracted` | Filled by extractor (0018+) |
| `partial` | Some fields present; errors on `item_errors` |

APIs accept any status string; these are the recommended stable values.

## Logical hash v1

Module: `logical_hash` (`LOGICAL_HASH_VERSION = 1`).

- Stored `logical_hash` = lowercase hex SHA-256 of a **versioned length-prefixed preimage**.
- Stored `logical_hash_version` matches the algorithm used (0 until computed).
- **BCC is always in the email frame** (empty list allowed). BCC-present ≠ BCC-absent.
- Does **not** include: raw MIME, PST property bags, CAS paths, matter ids, source paths, or the parent message’s own `native_sha256`.

### Email framing

```text
v1\n
message_id\n<len>\n<bytes>\n
subject\n<len>\n<bytes>\n
from\n<len>\n<bytes>\n
to\n<len>\n<bytes>\n
cc\n<len>\n<bytes>\n
bcc\n<len>\n<bytes>\n
sent\n<len>\n<bytes>\n
received\n<len>\n<bytes>\n
body\n<len>\n<bytes>\n
attachments\n<count>\n
  filename\n<len>\n<bytes>\n
  size\n<decimal>\n
  native_sha256\n<len>\n<bytes>\n
```

Address lists are sorted, case-folded, joined by `\n` **inside** the length-prefixed payload. Attachments sorted by `(filename_lower, size, native_sha256)`.

### Non-email framing

```text
v1\n
category\n… title\n… author\n… created\n… text\n…
children\n<count>\n
  native_sha256\n…
```

### Normalization highlights

| Input | Rule |
|---|---|
| Message-ID | Trim; strip `<>`; lowercase (parity with `dedup-engine`, duplicated — no crate coupling) |
| Subject (strict) | Collapse whitespace; **keep** `RE:`/`FW:` |
| Addresses | Trim; lowercase; sort each list |
| Times | UTC second-resolution RFC3339 or empty |
| Body | Minimal HTML tag strip if looks like HTML; CRLF→LF; strip zero-width; trim |

### Desk `logical_hash` vs `dedup-engine` Tier 2

| | Tier 2 content hash (`dedup-engine`) | Desk `logical_hash` v1 |
|---|---|---|
| Body | Preview-oriented normalization | Full normalized body text |
| Attachments | `name:size` only | `name` + `size` + `native_sha256` |
| Subject | lowercased aggressively; RE/FW stripped | Strict (keep RE/FW) |
| BCC | not modeled | **required in preimage** |
| Use | CLI scan today | Matter dedupe / promote (0021+) |

**Do not** silently rename Tier 2 as `logical_hash`.

Public helpers: `compute_email_logical_hash`, `compute_non_email_logical_hash`, `email_logical_preimage`, `normalize_message_id`, etc.

## Item APIs

| API | Purpose |
|---|---|
| `insert_item(ItemInput)` | All P0 fields optional; default `role=standalone`, `logical_hash_version=0` |
| `update_item(id, ItemUpdate)` | Partial update: nested `Option` = set when outer `Some` (inner `None` → SQL NULL); plain `None` → leave unchanged |
| `item_by_source_path` / `list_items_for_source` | 0016 inventory / resume |
| `items_by_logical_hash` / `items_by_message_id` | Prep for 0021 |

## Audit chain

- Append-only API (no update/delete of history).
- `prev_hash` for `seq=1` is the genesis sentinel (64 zero hex digits).
- `entry_hash` = SHA-256 of the canonical LF-separated encoding of  
  `(seq, ts, actor, action, entity, params, tool_version, prev_hash)`.
- `verify_audit_chain(conn)` walks and fails on break/tamper.
- Mutating high-volume `update_item` is **silent** on audit (extractors may batch); `family.create` is audited.

## Jobs / checkpoints

- Create job → transition state (`pending` / `running` / `paused` / `failed` / `cancelled` / `succeeded`).
- Upsert checkpoint by `(job_id, stage)` with opaque `cursor_json`.

## Quick use

```rust
use matter_core::{
    compute_email_logical_hash, item_role, item_status, EmailLogicalInput, ItemInput,
    ItemUpdate, Matter, LOGICAL_HASH_VERSION,
};

let m = Matter::create("Matters/demo", "Demo")?;
let fam = m.insert_family("")?; // email_attachments
let parent = m.insert_item(ItemInput {
    status: item_status::EXTRACTED.into(),
    role: Some(item_role::PARENT.into()),
    family_id: Some(fam.id.clone()),
    subject: Some("Hello".into()),
    ..Default::default()
})?;
let hash = compute_email_logical_hash(&EmailLogicalInput {
    message_id: None,
    subject: Some("Hello".into()),
    from: None,
    to: vec![],
    cc: vec![],
    bcc: vec![],
    sent: None,
    received: None,
    body: Some("hi".into()),
    attachments: vec![],
});
m.update_item(&parent.id, ItemUpdate {
    logical_hash: Some(Some(hash)),
    logical_hash_version: Some(LOGICAL_HASH_VERSION),
    status: Some(item_status::NORMALIZED.into()),
    ..Default::default()
})?;
```

## Compatibility with 0016

- Inventory rows use minimal `ItemInput` (`path`, `native_sha256`, status, `size_bytes`).
- Extended columns stay null until 0018+ extractors fill them.
- Resume via `(source_id, path)` unchanged (no unique index yet).

## Out of scope

Purview/PST parsing, bulk rehash jobs, review UI polish, multi-tenant, replacing `dedup-engine` CLI Tier 1/2. (Encryption at rest shipped in **0057**.)
