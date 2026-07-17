# matter-core

Library crate that owns the on-disk **matter** store for Dedupe Desk:

1. Matter directory layout + SQLite metadata (`matter.db`)
2. Content-addressable blob store (CAS) for **raw physical bytes**
3. Append-only audit log with integrity hash chain
4. Jobs + checkpoints for resumable work
5. Item-level error accumulator (`item_errors`)
6. **Normalized Item** model (schema **v2**) + family graph
7. Pure **logical_hash v1** helpers (length-prefixed preimage; BCC-aware)

Schema version: **2** (`SCHEMA_VERSION`).

## Layout

Under a caller-chosen root:

```text
<matter-root>/
  matter.db                 # SQLite (WAL, foreign_keys ON)
  blobs/sha256/<aa>/<hex>   # CAS — two-hex shard prefix
  index/                    # reserved for Tantivy (track 0029)
  exports/                  # reserved for production (track 0040)
  logs/                     # optional file logs
```

## CAS contract

| Decision | Choice |
|---|---|
| Algorithm | SHA-256, lowercase hex digest |
| Input | **Raw physical bytes only** (never normalized/logical content) |
| Path | `blobs/sha256/<aa>/<fullhex>` where `<aa>` is the first two hex chars |
| Collision | Existing path with different content → hard error (no overwrite) |
| Logical preimage | **Never** stored in CAS as “native”; only digests of body text may be stored as ordinary CAS blobs referenced by `text_sha256` / `html_sha256` |

## Schema v2 — Normalized Item (P0)

Extends v1 `items` with nullable columns (safe `ALTER TABLE … ADD COLUMN` migration from v1).

| Field | Notes |
|---|---|
| *(v1 columns)* | id, matter_id, source_id, family_id, path, native_sha256, logical_hash, message_id, status, size_bytes, timestamps |
| `role` | `standalone` \| `parent` \| `attachment` (constants in `item_role`) |
| `parent_item_id` | Denorm parent link; **app-enforced** (no ALTER FK) |
| `mime_type` | Best-effort IANA type |
| `file_category` | Coarse: `email`, `attachment`, `office`, `pdf`, … |
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

### Address storage (JSON decision)

P0 keeps `to_addrs_json` / `cc_addrs_json` / `bcc_addrs_json` on `items` as JSON arrays.

| Concern | Decision |
|---|---|
| Ingest / extract write path | JSON arrays match extractor output |
| Free-text / fielded participant search | **Tantivy (0029)** is plan-of-record — not SQLite JSON1 |
| Comms graphs (0038, 0047) | May add relational `item_participants` later; **not** assumed here |

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

Purview/PST parsing, bulk rehash jobs, Tantivy, review UI, encryption at rest, multi-tenant, replacing `dedup-engine` CLI Tier 1/2.
