# Internal Review R1 — 0017-NormalizedItem

## Verdict: NEEDS_FIXES

Solid core delivery: schema v2 migration, P0 `Item`/`ItemInput`/`ItemUpdate`, family APIs, length-prefixed BCC-aware logical hash, ingest-purview compatibility shape, and README coverage all land. Blocking-ish issues are **attachment_count maintenance** (insert / reparent / clear paths), **v2 multi-statement migration non-atomicity**, and a few **§3.7 proof gaps / contract mismatches**. DoD-7/8 gates were not observed as green in this review session (static review only; no `cargo`/`git` shell available to the reviewer).

## DoD Matrix

| Requirement | Status | Evidence |
|---|---|---|
| **DoD-1 — Schema v2** | **Met** | `SCHEMA_VERSION == 2` in `crates/matter-core/src/schema.rs`; `MIGRATION_V2` ADD COLUMN + `idx_items_logical_hash` / `idx_items_message_id`; unit tests `migrate_fresh_db_to_current`, `migrate_v1_inventory_to_v2_preserves_rows`, `migrate_resyncs_matters_schema_version_column`; integration `schema_v2_on_create`, `migrate_resyncs_matters_schema_version`. v1 inventory path/status/native retained; `logical_hash_version` defaults to 0. |
| **DoD-2 — Item model** | **Met** (with doc nit) | Public `Item` / `ItemInput` / `ItemUpdate` expose §3.2 P0 fields; `insert_item` defaults `role=standalone`, `logical_hash_version=0`; `update_item` partial nested-`Option` semantics implemented via `apply_opt2`. Integration `insert_update_item_normalized_fields` proves insert → update subject/hash → preserve BCC JSON. |
| **DoD-3 — Family graph** | **Partial** | `insert_family` (default `email_attachments`), `get_family`, `list_family_members`, `set_item_family_role`, `list_attachments`, `get_parent` present. Integration `family_parent_two_attachments` covers parent+2 children, roles, parent_item_id, list, get_parent, missing-parent reject. **Gap:** `attachment_count` only recomputed in `set_item_family_role` when a *new* parent is set — not on `insert_item(parent_item_id=…)`, not on reparent/clear; test itself admits stale count after insert. Cross-matter reject exists in code, **no test**. |
| **DoD-4 — Logical hash** | **Met** (test gaps) | `logical_hash.rs`: `LOGICAL_HASH_VERSION=1`, length-prefixed framing, BCC always framed (`bcc\n0\n\n`), sorted addrs/attachments, RE kept, body adversarial framing, non-email path, message-id normalize. Unit tests cover stability, sensitivity, BCC distinctness, adversarial body, RE, mid normalize, non-email smoke. **Gaps:** §3.7.9 transport-independence test is vacuous; native≠logical better covered in integration than unit test. |
| **DoD-5 — Compatibility** | **Likely met** (not executed here) | `ingest-purview/src/expand.rs` uses `ItemInput { …, ..Default::default() }` — correct inventory-only shape; no forced Normalized fields. **Not observed:** `cargo test -p ingest-purview` in this session. |
| **DoD-6 — Docs** | **Met** (one rustdoc error) | `crates/matter-core/README.md` documents schema v2 fields, family, framing + BCC, JSON address decision, status strings, Tier-2 distinction, migration notes. Root `README.md` / `ARCHITECTURE.md` note v2 + logical_hash. **Bug:** `ItemUpdate` rustdoc claims no nested Option / cannot set NULL — code + README contradict that. |
| **DoD-7 — Workspace gate** | **Not verifiable here** | Authorized to run gates; this reviewer had no shell. Static review only. Orchestrator must capture `fmt` / `clippy -D warnings` / `test --workspace` / `ledgerful verify`. |
| **DoD-8 — Recorded** | **Unmet** (expected pre-finalize) | No canonical `review.md`; conductor not marked Completed; ledger TX not in scope of this R1. |
| Spec §3.2 columns/indexes | **Met** | All P0 columns in `MIGRATION_V2`; required indexes created. Optional unique `(source_id, path)` correctly deferred. |
| Spec §3.2.4 migration mechanics | **Partial** | Prefer ADD COLUMN ✓; parent_item_id app-enforced ✓; matters.schema_version sync ✓; v1 fixture test ✓. **No transaction** around multi-statement v2 batch → crash mid-migrate can brick reopen (duplicate column). |
| Spec §3.3 family semantics | **Partial** | Create/list/link ✓; same-matter checks on family in insert/update/set ✓; parent existence on set ✓. attachment_count denorm incomplete (see findings). |
| Spec §3.4 algorithm | **Met** | Versioned length-prefixed email + non-email; BCC required; forbidden fields omitted from preimage (no matter/path/CAS/parent native). |
| Spec §3.5 status vocabulary | **Met** | `item_status` constants; free-string APIs. |
| Spec §3.6 APIs | **Met** | Minimum surface + optional `items_by_logical_hash` / `items_by_message_id`. Audit: `family.create` audited; `update_item` silent (documented). |
| Spec §3.7 tests | **Partial** | 1–8, 10–12 largely present; §3.7.9 transport independence weak; §3.7.11 integration-level OK; no cross-matter test; no index-presence assertion. |
| Spec §3.8 docs | **Met** | matter-core README + root notes. |
| Plan Phase 4 cross-matter | **Partial** | Implemented, untested. |
| Plan Phase 4 attachment_count bump | **Partial** | Only on `set_item_family_role` with `Some(parent)`. |

## Findings

### [P2] `attachment_count` not maintained on `insert_item` / reparent / clear

**Location:** `C:\dev\Dedupe\crates\matter-core\src\matter.rs` — `insert_item` (~505–579), `update_item` (~585–681), `set_item_family_role` (~817–858), `recompute_attachment_count` (~882–893)

**Problem:** Parent `attachment_count` is only recomputed inside `set_item_family_role` when `parent_item_id` is `Some`. Three production paths leave a wrong denorm count:

1. **`insert_item` with `parent_item_id: Some(…)`** — child is linked; parent count never bumped.
2. **Reparent** via `set_item_family_role` or `update_item` — new parent may be recomputed (set path only); **old parent is never decremented**.
3. **Clear parent** (`parent_item_id: None`) — no recompute of the previous parent.

**Evidence:** Integration test explicitly works around insert staleness:

```450:458:C:\dev\Dedupe\crates\matter-core\tests\integration.rs
    // Also recompute after att1 (inserted with parent already set but count may be stale).
    matter
        .set_item_family_role(
            &att1.id,
            Some(&family.id),
            item_role::ATTACHMENT,
            Some(&parent.id),
        )
        .expect("relink att1");
```

`insert_item` ends with `self.get_item(&id)` only — no `recompute_attachment_count`. `set_item_family_role` does not read the child’s previous `parent_item_id` before UPDATE.

**Required fix:**
- After successful insert when `parent_item_id` is set, call `recompute_attachment_count(parent)`.
- On `set_item_family_role` / `update_item` parent changes: recompute **old** and **new** parents when they differ.
- Add tests: insert-with-parent alone yields `attachment_count == 1`; reparent decrements old / increments new; clear parent zeros old count.

---

### [P2] Schema v2 migration is non-atomic (crash → unrecoverable reopen)

**Location:** `C:\dev\Dedupe\crates\matter-core\src\schema.rs` — `migrate` (~149–187), `MIGRATION_V2` (~124–147)

**Problem:** `migrate` runs `conn.execute_batch(sql)?` then updates `schema_meta`. SQLite auto-commits each DDL statement unless wrapped in an explicit transaction. `MIGRATION_V2` is ~18 `ALTER TABLE … ADD COLUMN` + 2 `CREATE INDEX`. If the process dies after some ALTERs but before `UPDATE schema_meta SET version = 2`, the DB remains at schema_meta=1 with partial columns. Next open re-runs v2 → `duplicate column name: role` (or similar) → matter permanently unopenable without manual repair.

**Evidence:** No `BEGIN`/`COMMIT` around migration steps; version bump is a separate execute after the batch.

**Required fix:** Wrap each migration step (batch + schema_meta update) in a single transaction (`BEGIN IMMEDIATE` … `COMMIT`), or make v2 idempotent (`ADD COLUMN` only if missing / tolerate duplicate column). Prefer transaction + a test that simulates partial apply if practical.

---

### [P2] §3.7.9 / native-vs-logical unit proof is vacuous

**Location:** `C:\dev\Dedupe\crates\matter-core\src\logical_hash.rs` — `native_sha256_not_in_email_logical_fields` (~562–575)

**Problem:** Spec §3.7.9 requires transport independence (same logical fields, different “MIME wrapper” notes → same hash). §3.7.11 requires same logical fields with different **message** `native_sha256` → same logical_hash. The unit test clones identical `sample_email()` twice and asserts equality — that would pass even if parent native were hashed. It does not construct two distinct transport wrappers or distinct message natives outside `EmailLogicalInput`.

**Evidence:** Test body is `let a = sample_email(); let b = sample_email(); assert_eq!(hash(a), hash(b))` with comments only.

**Mitigating evidence:** Integration `native_vs_logical_hash_independence` persists two items with different `native_sha256` and the same stored `logical_hash` — partially covers §3.7.11 at the store layer, not the pure-hash “transport notes” claim.

**Required fix:** Strengthen unit test, e.g. two `EmailLogicalInput`s equal in all logical fields (document that MIME/Received/headers are intentionally absent from the input type) and assert hash equality; optionally golden-assert preimage does not contain a synthetic “Received:” string if body is clean. Keep integration native≠logical test.

---

### [P2] `ItemUpdate` rustdoc contradicts implementation and README

**Location:** `C:\dev\Dedupe\crates\matter-core\src\matter.rs` ~164–169 vs fields ~171–200 and `apply_opt2` ~1011–1017; README Item APIs table

**Problem:** Doc comment states each `Option` is set when `Some`, and **“There is no nested Option for set to NULL”**. The type is `Option<Option<T>>` for nearly every field; `apply_opt2` treats outer `None` = leave, `Some(None)` = SQL NULL. README correctly documents nested Option. Callers (0018) following rustdoc will believe they cannot clear fields and may misuse the API.

**Evidence:**

```164:169:C:\dev\Dedupe\crates\matter-core\src\matter.rs
/// Semantics: each `Option` field is **set when `Some`**; `None` leaves the
/// column unchanged. There is no nested `Option` for “set to NULL” in this
/// track — callers that need to clear a column can pass empty string / re-insert
/// later; 0018 extractors typically only set fields they know.
```

vs `pub path: Option<Option<String>>` and `apply_opt2`.

**Required fix:** Replace rustdoc with the nested-Option contract matching README/code. Optionally add a one-line unit/integration test that `Some(None)` clears e.g. `subject`.

---

### [P3] Cross-matter family reject untested

**Location:** `matter.rs` `insert_item` / `update_item` / `set_item_family_role`; `error.rs` `CrossMatterFamily`

**Problem:** Plan Phase 4 and error type implement cross-matter refusal, but no test forces a family/parent with a foreign `matter_id` (raw SQL insert into same DB is enough).

**Required fix:** Unit/integration: insert a second matter + family via SQL (or second root if multi-DB), attempt assignment, expect `Error::CrossMatterFamily`.

---

### [P3] `insert_item` does not check parent `matter_id` (inconsistent with `set_item_family_role`)

**Location:** `matter.rs` ~515–520 vs ~836–846

**Problem:** `set_item_family_role` rejects parent with different `matter_id`; `insert_item` only checks parent exists via `get_item`. Under the normal one-matter-per-DB layout this is hard to hit, but the two APIs are inconsistent if multi-matter rows ever share a connection.

**Required fix:** After resolving parent, assert `parent.matter_id == self.matter_id` (same as set path).

---

### [P3] Migrated v1 inventory rows keep `role = NULL` (not `standalone`)

**Location:** `schema.rs` `MIGRATION_V2` (`role TEXT` nullable); `migrate_v1_inventory_to_v2_preserves_rows` asserts role is None

**Problem:** Spec §3.3 says standalone items use `role=standalone`. New inserts default correctly; migrated 0016 rows stay NULL. Acceptable for nullable migration, but 0018/UI must treat NULL like standalone or a one-time backfill is needed.

**Required fix (optional):** Document “NULL role ≡ standalone for pre-v2 rows” in README, or `UPDATE items SET role = 'standalone' WHERE role IS NULL` in v2 migration.

---

### [P3] No assertion that v2 indexes exist after migrate

**Location:** `schema.rs` tests

**Problem:** Columns are asserted; `idx_items_logical_hash` / `idx_items_message_id` are not. Unlikely to regress silently if SQL is static, but cheap to check via `sqlite_master`.

**Required fix:** Query `sqlite_master` for both index names in the v1→v2 migration test.

---

## Completeness sweep

| Check | Result |
|---|---|
| TODO/FIXME/todo!/unimplemented in 0017 surfaces | None material (`jobs.rs` “placeholder; set below” is pre-existing state parse, not a stub). |
| Logical hash pure API surface | Present and re-exported from `lib.rs`. |
| ingest-purview call site | `..Default::default()` — correct; inventory stays minimal. |
| Silent wrong defaults | `role`/`logical_hash_version` defaults correct on insert; migrated role NULL (P3). |
| Placeholders masquerading as complete | No fake hash/algorithm stubs. |
| Forbidden scope (PST/EML parse, bulk rehash, Tier-2 replace) | Not present — good. |

## Wiring / regression notes

- **0018 handoff path:** extract → CAS native → `insert_item`/`update_item` fields (incl. BCC JSON) → family link → `compute_email_logical_hash` → store hash + `LOGICAL_HASH_VERSION` is supported by public API.
- **Risk for 0018:** if extractors insert attachments with `parent_item_id` set and trust `attachment_count`, counts will be wrong until P2 fix.
- **CAS:** still physical-only; logical preimage not stored as native — correct.
- **Indexes for 0021:** logical_hash / message_id indexes present in SQL.

## Verification evidence

| Command | Observed this session |
|---|---|
| Static read of spec/plan + listed implementation files | Yes |
| `git log main..HEAD` / `git diff main...HEAD` | **Not run** (no shell) |
| `cargo test -p matter-core` | **Not run** |
| `cargo test -p ingest-purview` | **Not run** |
| Workspace fmt/clippy/test / `ledgerful verify` | **Not run** |

Orchestrator should re-run gates after P2 fixes and attach evidence to canonical `review.md`.

## Summary

Implementation quality is high for a schema+API track: migration fixture, family graph happy path, and the hard logical-hash properties (length-prefix framing, BCC distinctness, RE kept, adversarial body) are real and mostly well tested. **Do not mark Completed until:**

1. **attachment_count** is maintained for insert/reparent/clear (or the denorm is documented as set-role-only and insert path is forbidden for parent links — prefer fix).
2. **v2 migrate** is transactional or idempotent.
3. **ItemUpdate** rustdoc matches nested-Option reality.
4. Strengthen the weak native/transport hash unit test.
5. Observe green **matter-core + ingest-purview + workspace gates + ledgerful verify**.

**Recommended disposition after fixes:** re-run internal review; expect CLEAN if P2s closed and gates observed.
