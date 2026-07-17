# 0017-NormalizedItem — Review

- **Track:** 0017-NormalizedItem
- **Status:** Completed — review loop clean (PASS after family-cohesion fix)
- **Date:** 2026-07-17
- **Schema version:** **2** (`matter_core::SCHEMA_VERSION`)
- **Logical hash version:** **1** (`LOGICAL_HASH_VERSION`)

## Summary

Extended **`matter-core`** with the Desk Normalized Item model:

| Area | Result |
|---|---|
| Schema | v1 → v2 migration (nullable ADD COLUMN + indexes); transactional per-step migrate |
| Item model | P0 fields on `Item` / `ItemInput` / `ItemUpdate` (nested Option for clear-to-NULL) |
| Family graph | `insert_family`, `list_family_members`, `set_item_family_role`, attachments helpers |
| Cohesion | Parent/child must share `family_id` (or inherit parent family); cross-matter rejected |
| `attachment_count` | Recomputed on insert/update/set_item_family_role (reparent + clear) |
| Logical hash | Length-prefixed email + non-email preimage; **BCC always framed**; pure helpers |
| Compatibility | 0016 inventory rows migrate intact; `ingest-purview` uses `ItemInput { ..Default::default() }` |

## Public API (overview)

- Constants: `SCHEMA_VERSION=2`, `LOGICAL_HASH_VERSION=1`, `item_status::*`, `item_role::*`, `FAMILY_KIND_EMAIL_ATTACHMENTS`
- Types: `Item`, `ItemInput`, `ItemUpdate`, `ItemFamily`, `EmailLogicalInput`, `NonEmailLogicalInput`, `LogicalAttachment`
- Matter: `insert_item`, `update_item`, `get_item`, `item_by_source_path`, `list_items_for_source`, `items_by_logical_hash`, `items_by_message_id`
- Family: `insert_family`, `get_family`, `list_family_members`, `set_item_family_role`, `list_attachments`, `get_parent`
- Hash: `compute_email_logical_hash`, `compute_non_email_logical_hash`, `email_logical_preimage`, normalize helpers

## Logical hash framing (v1)

```text
v1\n + length-prefixed fields in fixed order
email: message_id, subject, from, to, cc, bcc, sent, received, body, attachments
non-email: category, title, author, created, text, children digests
```

BCC-present ≠ BCC-absent. Body cannot spoof attachment structure (length-prefixed). Strict subject keeps `RE:`/`FW:`. Distinct from CLI Tier-2 content hash.

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all --check` | **PASS** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** |
| `cargo test -p matter-core` | **PASS** (16 unit + 15 integration) |
| `cargo test -p ingest-purview` | **PASS** (13 unit + 12 integration) |
| `cargo test --workspace` | **PASS** |
| `ledgerful verify` | **PASS** (fmt + clippy + test) |

### Manual / targeted evidence

```text
cargo test -p matter-core --test integration family_cohesion  → ok
cargo test -p matter-core logical_hash::tests::bcc_distinctness → ok
```

Covers: BCC distinctness, family cohesion reject, attachment_count after insert.

### Required tests covered (§3.7)

1. Migration v1 inventory → v2 preserves rows + indexes  
2. Family parent + 2 attachments; parent_item_id; list members  
3. Hash stability / sort independence  
4. Sensitivity (body / attachment digest)  
5. BCC distinctness  
6. Adversarial body framing  
7. RE: kept in subject  
8. Message-ID normalize parity  
9. Native ≠ logical (message native not in email preimage)  
10. Non-email smoke  
11. Family cohesion mismatches rejected  

## Review loop

| Round | Agent | Verdict | Notes |
|---|---|---|---|
| Implement | general-purpose | — | Schema v2 + family + logical_hash |
| Internal R1 | general-purpose | NEEDS_FIXES | attachment_count, atomic migrate, weak test, rustdoc |
| Fix R1 | general-purpose | — | All P2 fixed |
| Internal R2 | general-purpose | CLEAN | Prior P2 verified |
| Codex R1 | gpt-5.6-luna high | FAIL | P2 family_id cohesion unenforced; DoD-8 not yet finalized |
| Fix Codex | general-purpose | — | Family cohesion + tests + README |
| Codex R2 | gpt-5.6-luna high | **PASS WITH DEFERRED P3** | Cohesion verified; only D-0017-01..05 intentional deferrals |

## Deferred (see `docs/deferred.md`)

| ID | Item |
|---|---|
| D-0017-01 | Unique index on `(source_id, path)` still optional |
| D-0017-02 | Formal SQLite FK on `parent_item_id` (app-enforced only) |
| D-0017-03 | Relational `item_participants` (Tantivy/graph tracks) |
| D-0017-04 | Body-to-CAS promote helper for `text_sha256` |
| — | PST/EML fill → **0018**; bulk process → **0019** |

Closed from 0016 backlog: **D-0016-07** Full Normalized Item model (this track).

## Unblocked

**0018** PstExtractorAdapter (with 0016). **0021** can plan on `logical_hash` / `message_id` indexes.

## Artifacts

- `crates/matter-core/src/schema.rs`, `matter.rs`, `logical_hash.rs`, `error.rs`, `lib.rs`
- `crates/matter-core/README.md`, `tests/integration.rs`
- `crates/ingest-purview/src/expand.rs` (Default ItemInput)
- `ARCHITECTURE.md`, root `README.md`
- Internal reviews: `review.subagent-r1.md`, `review.subagent-r2.md`, `review.codex.md`
