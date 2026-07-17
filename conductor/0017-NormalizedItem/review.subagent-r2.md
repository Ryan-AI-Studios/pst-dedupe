# Internal Review R2 — 0017-NormalizedItem

## Verdict: CLEAN

All R1 P2 findings are fixed in production code with matching tests/docs. R1 P3s are addressed (tests + same-matter checks + README / index assertions). No new P0/P1/P2 defects found from the fix pass. Residual items are process gates (DoD-7 package run / DoD-8 finalize) and optional nits only.

## Prior findings disposition

| R1 ID | Finding | Disposition | Evidence |
|---|---|---|---|
| **[P2]** `attachment_count` not maintained on insert / reparent / clear | **Fixed** | `insert_item` calls `recompute_attachment_count` when `parent_item_id` set (`matter.rs` ~592–594). `update_item` captures `old_parent`, recomputes old + new when parent changes (~604–715). `set_item_family_role` recomputes old/new on change and still recomputes on same-parent reaffirm (~890–902). Integration: insert path asserts count==1 immediately (`family_parent_two_attachments`); `attachment_count_reparent_and_clear` covers reparent A→B and clear via `update_item` `Some(None)`. Stale-count workaround from R1 removed. |
| **[P2]** Schema v2 migration non-atomic | **Fixed** | `migrate()` wraps each step (batch + `schema_meta` bump) in `BEGIN IMMEDIATE` … `COMMIT`, with `ROLLBACK` on error (`schema.rs` ~149–179). Doc comment states crash-mid-batch rationale. Unit smoke `migrate_steps_are_transactional` + existing v1→v2 fixture. |
| **[P2]** §3.7.9 / native≠logical unit test vacuous | **Fixed (adequate)** | `native_sha256_not_in_email_logical_fields` now documents distinct would-be message natives + MIME wrappers, asserts hash equality for identical logical inputs, and asserts preimage lacks `Received:`, `MIME-Version`, and the synthetic message natives while still framing attachment `native_sha256` (`logical_hash.rs` ~562–603). Integration `native_vs_logical_hash_independence` still proves store-layer same-hash / different-native. |
| **[P2]** `ItemUpdate` rustdoc contradicts nested Option | **Fixed** | Rustdoc documents outer `None` / `Some(None)` / `Some(Some(v))` + plain-Option exceptions for `status` / `logical_hash_version` (`matter.rs` ~164–175). Matches README. Integration `item_update_some_none_clears_subject` proves clear path. |
| **[P3]** Cross-matter family reject untested | **Fixed** | Integration `cross_matter_family_rejected`: foreign matter/family/parent injected via SQL; insert family, set_role family, set_role parent, insert parent all expect `Error::CrossMatterFamily`. |
| **[P3]** `insert_item` no parent same-matter check | **Fixed** | `insert_item` rejects parent with `parent.matter_id != self.matter_id` (~526–531); also rejects foreign family (~533–540). Covered by cross-matter test. |
| **[P3]** Migrated v1 `role = NULL` | **Documented (acceptable)** | README: “NULL `role` ≡ standalone” for pre-v2 inventory; family semantics note the same. Migration test still asserts NULL (no forced backfill — intentional nullable migrate). |
| **[P3]** No v2 index presence assertion | **Fixed** | `migrate_v1_inventory_to_v2_preserves_rows` queries `sqlite_master` for `idx_items_logical_hash` and `idx_items_message_id`. |

## DoD Matrix

| Requirement | Status | Evidence |
|---|---|---|
| **DoD-1 — Schema v2** | **Met** | `SCHEMA_VERSION == 2`; transactional `MIGRATION_V2` ADD COLUMN + indexes; v1 inventory fixture preserves path/status/native; `matters.schema_version` re-synced; indexes asserted. |
| **DoD-2 — Item model** | **Met** | Public `Item` / `ItemInput` / `ItemUpdate` P0 fields; insert defaults `role=standalone`, `logical_hash_version=0`; partial nested-Option update + clear; integration insert/update + clear-subject. |
| **DoD-3 — Family graph** | **Met** | Create family; parent+2 attachments; roles/`parent_item_id`; list members/attachments/`get_parent`; missing parent reject; **attachment_count** maintained on insert/set/reparent/clear; cross-matter refuse + tests. |
| **DoD-4 — Logical hash** | **Met** | Length-prefixed email/non-email; `LOGICAL_HASH_VERSION=1`; BCC always framed; unit tests for stability/sensitivity/BCC/framing/RE/mid normalize/transport+native independence/non-email; integration native≠logical store proof. |
| **DoD-5 — Compatibility** | **Likely met** (not executed here) | `ingest-purview` uses `ItemInput { …, ..Default::default() }` inventory-only shape. Static compile surface correct. **`cargo test -p ingest-purview` not observed this session** (no shell). |
| **DoD-6 — Docs** | **Met** | matter-core README: schema v2 fields, JSON address decision, migration notes (NULL role, transactional migrate, attachment_count recompute), family, framing+BCC, status, Tier-2 distinction, ItemUpdate nested Option. Root README + ARCHITECTURE note v2 / logical_hash. ItemUpdate rustdoc aligned. |
| **DoD-7 — Workspace gate** | **Not verifiable here** | Reviewer has no shell; package/workspace/`ledgerful verify` not observed. Orchestrator must capture `fmt` / `clippy -D warnings` / `test --workspace` / `ledgerful verify`. |
| **DoD-8 — Recorded** | **Unmet** (expected pre-finalize) | No canonical `review.md`; conductor not Completed; ledger TX orchestrator-owned. |
| Spec §3.2 / §3.2.4 | **Met** | P0 columns + indexes; ADD COLUMN prefer; app-enforced parent; transactional steps; v1 fixture. |
| Spec §3.3 family | **Met** | Create/list/link; same-matter; attachment_count denorm maintained on all public parent-link paths. |
| Spec §3.4 algorithm | **Met** | Versioned length-prefix; BCC; forbidden fields omitted from preimage. |
| Spec §3.5–3.6 APIs | **Met** | Status constants; min + optional hash/message_id queries; family.create audited; update silent (documented). |
| Spec §3.7 tests | **Met** | 1–12 covered at unit and/or integration level; transport independence strengthened with preimage asserts. |
| Spec §3.8 docs | **Met** | As DoD-6. |

## New findings (if any)

**None at P0/P1/P2.**

### Nits (non-blocking; do not reopen R1)

1. **`migrate_steps_are_transactional` is smoke-only** — proves completed migrate leaves version/columns consistent; does not inject a mid-batch crash. Code path (`BEGIN IMMEDIATE` + rollback) is correct; crash injection is optional hardening.
2. **`update_item` cross-matter parent/family** — same checks as insert/set_role, but only insert/set_role are integration-tested. Low risk.
3. **Insert + recompute not one SQLite transaction** — child row can exist with briefly stale parent count if recompute fails after INSERT. Normal single-connection use is fine; not a regression vs R1 fix scope.
4. **Package tests not run in this review session** — static evidence only for DoD-5/7.

## Completeness / wiring (re-check)

| Check | Result |
|---|---|
| TODO/FIXME/todo!/unimplemented on 0017 surfaces | None material (`jobs.rs` “placeholder; set below” pre-existing state parse). |
| Forbidden scope (PST/EML parse, bulk rehash, Tier-2 replace) | Absent — good. |
| ingest-purview inventory path | `..Default::default()` — correct. |
| 0018 handoff | insert/update fields + family + `compute_email_logical_hash` + store hash/version still supported; attachment_count now trustworthy on insert-with-parent. |
| CAS | Physical-only; logical preimage not stored as native. |

## Verification evidence

| Command | Observed this session |
|---|---|
| Static re-read of prior R1 + claimed fix paths | Yes |
| `matter.rs` / `schema.rs` / `logical_hash.rs` / integration / README | Yes |
| `cargo test -p matter-core` | **Not run** (no shell in reviewer environment) |
| `cargo test -p ingest-purview` | **Not run** |
| Workspace fmt/clippy/test / `ledgerful verify` | **Not run** |

Orchestrator should re-run gates and attach results to canonical `review.md`.

## Summary

R2 closes the R1 blocking set:

1. **attachment_count** recomputed on insert, update reparent/clear, and set_item_family_role (with tests).
2. **Migrations** atomic per step via `BEGIN IMMEDIATE`.
3. **Native/transport unit proof** strengthened with preimage negative asserts.
4. **ItemUpdate** docs match nested-Option implementation; clear tested.
5. **Cross-matter** + insert parent same-matter enforced and tested.
6. **README** documents recompute, NULL role, transactional migrate.

**Engineering disposition: CLEAN** — no further code fixes required for 0017 internal review. Mark track **Completed** only after orchestrator observes green `cargo test -p matter-core`, `cargo test -p ingest-purview`, full workspace gates, `ledgerful verify`, and writes canonical `review.md` + conductor/ledger finalize (DoD-7/8).
