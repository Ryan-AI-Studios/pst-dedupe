# 0017 — Normalized Item model + family graph — Plan

Phased checklist. Map phases to DoD items in `spec.md` §7. Execute in `C:\dev\dedupe`.

> **Ledger:**  
> `ledgerful ledger start 0017-normalizeditem --category ARCHITECTURE --message "Normalized Item schema v2 + family graph + logical_hash v1"`  
> Prefer `ARCHITECTURE` (schema contract). Use `FEATURE` only if ledger policy prefers it. Commit in Finalize.

---

## Phase 0 — Preconditions → DoD-7 baseline

- [x] Confirm **0015** Completed: read `../0015-MatterStore/review.md`, `crates/matter-core/README.md`
- [x] Confirm **0016** Completed (recommended): read `../0016-PurviewIngest/review.md` — inventory path/status conventions
- [x] Read plan-of-record: `C:\dev\Dedupe-plan.md` §§2.2, **2.3**
- [x] `cargo test -p matter-core` green
- [x] `cargo test -p ingest-purview` green (compatibility baseline)
- [x] Note: no `item_families` API yet; `logical_hash` column unused; schema **v1**

## Phase 1 — Design lock → DoD-1/2/4 prep

- [x] Freeze P0 column list from `spec.md` §3.2 (no privilege/OCR/tags in v2)
- [x] Freeze status string constants
- [x] Freeze `LOGICAL_HASH_VERSION = 1` **length-prefixed** preimage bytes (exact tags/order — document; **include bcc**)
- [x] Confirm BCC policy: BCC-present ≠ BCC-absent for `logical_hash` (defensibility)
- [x] Confirm address storage decision: JSON on `items` P0; Tantivy for search; relational participants deferred (`spec.md` §3.2.3)
- [x] Decide address case-folding rules (document) for To/Cc/**Bcc**
- [x] Decide HTML→text minimal strategy for body used in hash (document limits)
- [x] Map `dedup-engine` Message-ID normalize: share helper vs duplicate + parity test
- [x] Sketch migration SQL v1→v2 per `spec.md` §3.2.4:
  - [x] Prefer nullable `ADD COLUMN` + `CREATE INDEX`
  - [x] No assumption that ALTER can add FKs; `parent_item_id` app-enforced if needed
  - [x] Table-rebuild plan only if a hard constraint forces it
- [x] API names: `insert_family`, `update_item`, `set_item_family_role`, `list_family_members`, logical hash fns
- [x] Audit policy: which mutations append events (avoid per-field spam)

## Phase 2 — Schema migration v2 → DoD-1

- [x] Bump `SCHEMA_VERSION` to **2** in `schema.rs`
- [x] Add `MIGRATION_V2` with new columns + indexes (validate SQLite ALTER limits on Windows)
- [x] Ensure `migrate()` keeps `matters.schema_version` in sync (0015 pattern)
- [x] Unit test: fresh DB lands on v2
- [x] Unit/integration: **v1 fixture data** (0016-style inventory rows) → open/migrate → columns present; data intact
- [x] If table rebuild is required, implement inside a transaction + test thoroughly
- [x] Keep ingest-purview compiling (update `ItemInput { .. }` literals if exhaustive)

## Phase 3 — Item CRUD extensions → DoD-2

- [x] Extend `Item` / `ItemInput` structs + row mappers
- [x] `insert_item` writes new fields (null-safe)
- [x] `update_item` (partial or full — prefer explicit `ItemUpdate` with Option fields)
- [x] Defaults: `role=standalone`, `logical_hash_version=0` until hash set
- [x] Tests: insert → get → update subject/logical_hash → get

## Phase 4 — Family graph → DoD-3

- [x] `insert_family(kind)`
- [x] `get_family`, `list_family_members`
- [x] Helper to attach children: set `family_id`, `role`, `parent_item_id`, bump parent `attachment_count`
- [x] Tests: parent + 2 attachments; list; get_parent
- [x] Reject cross-matter family assignment if cheap to check

## Phase 5 — Logical hash module → DoD-4

- [x] Module `logical_hash.rs` (or `normalize.rs`) in matter-core
- [x] Implement **length-prefixed** email + non-email preimage builders + SHA-256 hex
- [x] Include **bcc** field (empty list allowed; never omit from framing)
- [x] Implement normalize helpers (message_id, subject strict, To/Cc/Bcc addrs, body, times)
- [x] Unit tests from `spec.md` §3.7:
  - [x] stability / sensitivity / RE kept / native≠logical / attachment order independence
  - [x] **BCC distinctness**
  - [x] **adversarial body** containing attachment-like text does not alter structure
- [x] Optional: helper `apply_email_logical_hash(item fields) -> (hash, version)` for 0018 convenience
- [x] Document algorithm + framing + BCC policy + JSON address decision in matter-core README

## Phase 6 — Compatibility + docs → DoD-5, DoD-6

- [x] Fix any `ItemInput` construction sites (ingest-purview, tests)
- [x] `cargo test -p ingest-purview` — **must pass**
- [x] Update `crates/matter-core/README.md` (schema v2, family, logical hash v1 + framing + BCC, JSON vs participants decision, Tier-2 distinction, 0016 inventory compatibility)
- [x] Touch root `ARCHITECTURE.md` / `README.md` if item/matter sections need a line
- [x] Note for **0018**: expected fill path (extract → CAS native → fields → family → logical_hash)

## Phase 7 — Verification → DoD-7

- [x] `cargo test -p matter-core`
- [x] `cargo test -p ingest-purview`
- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `ledgerful verify` (**required**)
- [x] Capture evidence for `review.md`

## Phase 8 — Finalize → DoD-8

- [x] Write `review.md` (schema diff, API list, hash version, compatibility notes, deferred fields)
- [x] Update `../conductor.md`: **0017** → **Completed**
- [x] Update `../sequencing.md` markers
- [x] Commit ledger TX
- [x] Handoff: **0018** unblocked (with 0016); **0021** can plan on `logical_hash` / `message_id` indexes

---

## Suggested file map

```
crates/matter-core/
  src/
    schema.rs          # MIGRATION_V2, SCHEMA_VERSION=2
    matter.rs          # Item fields, update_item, family APIs
    family.rs          # optional extract of family helpers
    logical_hash.rs    # NEW pure hash + normalize
    lib.rs             # re-exports
  README.md            # document v2 + hash contract
  tests/
    integration.rs     # migration, family, hash integration if needed
```

No new workspace crate unless Phase 1 explicitly decides `normalize-core` (default: **no**).

---

## Default constants (starting point)

| Constant | Value |
|---|---|
| `SCHEMA_VERSION` | 2 |
| `LOGICAL_HASH_VERSION` | 1 |
| Default `role` | `standalone` |
| Family kind default | `email_attachments` |

---

## Handoff notes

- **0016 inventory rows** stay valid with null extended fields; do not require re-ingest.
- **0018** will: open PST → create/update items + families → set `native_sha256` for message/attachment blobs → set `text_sha256` → populate To/Cc/**Bcc** → call `compute_email_logical_hash`.
- **0021** will query `logical_hash` / `message_id` for matter dedupe; BCC-aware hashes prevent suppressing BCC-bearing copies.
- **0029 / 0038 / 0047:** participant search/graphs — do not assume SQL over JSON is enough; plan relational or Tantivy as those tracks require.
- Do **not** parse PST/EML in 0017.
- Do **not** change CAS to store logical preimages as natives.
- Single-exe / no-daemon invariant unchanged.
- Keep `ingest-purview` blocking-thread contract unchanged.
