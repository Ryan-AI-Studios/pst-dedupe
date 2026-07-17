# Track Completion Audit — 0017-NormalizedItem

## Verdict: FAIL

## Scope Reviewed

Reviewed `main...feat/0017-normalized-item`:

- `573219d`, `6970513`, `4ef5d6c`
- Full `spec.md` and `plan.md`
- Implemented matter-core, ingest-purview, tests, README, architecture, and governance files
- Untracked `review.subagent-r1.md` and `review.subagent-r2.md` as context only

## Requirement and DoD Matrix

| Requirement | Result |
|---|---|
| DoD-1 Schema v2 and v1 survival | Met |
| DoD-2 Item model and updates | Met |
| DoD-3 Family graph | Partial — family cohesion invariant is unenforced |
| DoD-4 Logical hash, framing, BCC | Met |
| DoD-5 ingest-purview compatibility | Met per supplied gate evidence |
| DoD-6 Documentation | Met, subject to family-validation correction |
| DoD-7 Workspace gate and `ledgerful verify` | Not established |
| DoD-8 Review/governance finalization | Not met |

## Findings

### [P1] Required completion evidence and governance finalization are absent

DoD-7/8 require workspace tests, `ledgerful verify`, `review.md`, a Completed conductor status, and a committed ledger transaction.

Evidence:

- `conductor/conductor.md:56` still marks 0017 as `Ready`.
- `conductor/sequencing.md:44` still marks it `Ready`.
- `conductor/0017-NormalizedItem/review.md` does not exist.
- Current `ledgerful` commands cannot open their database.
- Current Cargo verification is blocked by access denied on `target\debug\.cargo-lock`.
- The supplied evidence covers fmt, clippy, and package tests, but not a successful workspace test plus Ledgerful verification.

This is a completion blocker even though most implementation gates are reported green.

### [P2] Family APIs permit parent/child links across different families

The specification requires that parent and child items share `family_id` (`spec.md:165-170`).

The public APIs validate that the family and parent belong to the same matter, but never compare their family IDs:

- `insert_item` checks parent and family independently: `matter.rs:522-540`
- `update_item` checks them independently: `matter.rs:638-658`
- `set_item_family_role` checks them independently: `matter.rs:863-883`

A reachable invalid state is therefore:

1. Parent belongs to family A.
2. Child is assigned to family B with that parent.
3. `list_family_members(B)` returns the child while the parent remains in family A.

The APIs should reject mismatched family IDs, reject a parent link without a family, and add regression tests for insert, update, and `set_item_family_role`.

## Completeness Sweep

- No material stubs, fake hashes, no-op implementations, or forbidden PST/EML parsing found.
- Logical hashing is pure and re-exported through `matter-core`.
- BCC is always framed and included.
- Length-prefixed body framing handles embedded attachment-like text.
- CAS remains physical-byte-only.
- `ingest-purview` retains inventory-only behavior through `ItemInput { ..Default::default() }`.
- `ItemUpdate` nested `Option` semantics are documented and tested.
- Migration preserves v1 inventory rows and asserts new indexes.
- `attachment_count` recomputation covers insert, reparent, clear, and family-role updates.
- R2’s remaining notes—crash-injection testing and transaction-boundary hardening around recomputation—are non-blocking.

## Wiring and Regression Review

The 0018 handoff is reachable:

`extract → ItemInput/ItemUpdate → family link → logical hash → stored hash/version`

No PST/EML parsing or dedup-engine Tier-2 replacement leaked into this track.

The family graph is the remaining production correctness gap: family membership and parent linkage can diverge through all three mutation APIs.

## Verification Evidence

- `cargo fmt --all --check`: passed.
- `cargo clippy ...`: current run blocked by access denied to `target\debug\.cargo-lock`; supplied orchestrator evidence reports pass.
- `cargo test -p matter-core`: current run blocked by the same lock; supplied evidence reports 16 unit + 14 integration tests passing.
- `cargo test -p ingest-purview`: current run blocked by the same lock; supplied evidence reports 13 unit + 12 integration tests passing.
- `cargo test --workspace`: not independently established.
- `ledgerful verify`: failed because the Ledgerful database/report could not be opened and Cargo verification was blocked.
- `git diff --check`: reports existing Markdown trailing whitespace in the changed plan and prior review document.
- Working tree contains only the requested untracked reviews plus `codex-prompt.txt`; no tracked source changes were made.

## Deferred Candidates

None. The family invariant and completion records are blockers, not deferred P3 work.

## Completion Decision

FAIL. Fix family-ID cohesion validation and tests, then complete the workspace verification and DoD-8 governance finalization in a writable environment.

