# 0017 implementation notes (draft)

## Delivered

- `SCHEMA_VERSION = 2` with `MIGRATION_V2` (nullable ADD COLUMN + indexes)
- `Item` / `ItemInput` / `ItemUpdate` P0 fields; `item_status` / `item_role` constants
- Family graph: `insert_family`, `get_family`, `list_family_members`, `set_item_family_role`, `list_attachments`, `get_parent`
- `logical_hash` module: email + non-email, length-prefixed framing, BCC always present
- ingest-purview `ItemInput` via `..Default::default()` (inventory-only)
- Docs: matter-core README, ARCHITECTURE.md, root README, lib.rs

## Framing (email)

```
v1\n
message_id\n<len>\n<bytes>\n
subject\n… from\n… to\n… cc\n… bcc\n…
sent\n… received\n… body\n…
attachments\n<count>\n
  filename\n… size\n<decimal>\n native_sha256\n…
```

## Deferred (intentional / out of scope)

- Unique index `(source_id, path)`
- Body → CAS + `text_sha256` promote helpers
- PST/EML parse (0018)
- Bulk rehash / process jobs (0019)
- Replacing dedup-engine Tier 2
- Formal SQLite FK on `parent_item_id` (app-enforced)
- Relational `item_participants` (later tracks)

## Verify

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p matter-core
cargo test -p ingest-purview
cargo test --workspace
```

Do **not** mark conductor Completed / commit ledger from this note — orchestrator after review.
