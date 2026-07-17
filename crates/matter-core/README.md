# matter-core

Library crate that owns the on-disk **matter** store for Dedupe Desk:

1. Matter directory layout + SQLite metadata (`matter.db`)
2. Content-addressable blob store (CAS) for **raw physical bytes**
3. Append-only audit log with integrity hash chain
4. Jobs + checkpoints for resumable work
5. Item-level error accumulator (`item_errors`)

Schema version: **1** (`SCHEMA_VERSION`).

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
| Logical hash | **Not** in CAS; nullable column on `items` for later tracks |

## Audit chain

- Append-only API (no update/delete of history).
- `prev_hash` for `seq=1` is the genesis sentinel (64 zero hex digits).
- `entry_hash` = SHA-256 of the canonical LF-separated encoding of  
  `(seq, ts, actor, action, entity, params, tool_version, prev_hash)`.
- `verify_audit_chain(conn)` walks and fails on break/tamper.

## Jobs / checkpoints

- Create job → transition state (`pending` / `running` / `paused` / `failed` / `cancelled` / `succeeded`).
- Upsert checkpoint by `(job_id, stage)` with opaque `cursor_json`.
- Designed so ingest/process tracks can resume after crash.

## Quick use

```rust
use matter_core::{Matter, JobState};

let m = Matter::create("Matters/demo", "Demo")?;
let digest = m.put_bytes(b"raw evidence")?;
let job = m.create_job("ingest")?;
m.set_job_state(&job.id, JobState::Running, None)?;
m.put_checkpoint(&job.id, "expand", r#"{"offset":0}"#, 0)?;
m.verify_audit_chain()?;
```

## Out of scope

Purview/PST parsing, logical hash computation, Tantivy, review UI, encryption at rest, multi-tenant.
