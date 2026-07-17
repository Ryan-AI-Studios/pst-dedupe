# Track Completion Audit — 0015-MatterStore (Review Round 1)

## Verdict: PASS WITH DEFERRED P3

## Scope Reviewed

| Area | Paths |
|---|---|
| Track docs | `conductor/0015-MatterStore/spec.md`, `plan.md`, `review.md` |
| Implementation | `crates/matter-core/` |
| Workspace | Root `Cargo.toml`, `Cargo.lock` |
| Docs / board | `ARCHITECTURE.md`, `README.md`, `conductor/conductor.md`, `sequencing.md` |

## Requirement and DoD Matrix

| Requirement | Status | Gap |
|---|---|---|
| §3.1 Crate + workspace | Met | — |
| §3.2 Layout | Met | — |
| §3.3 All tables | Met | Dual schema_version (P3) |
| §3.4 CAS physical + no clobber | Met | — |
| §3.5 Audit chain | Met | — |
| §3.6 Jobs/checkpoints | Met | — |
| §3.7 Item errors | Met | — |
| §3.8 Five required tests | Met | — |
| DoD-1…DoD-8 | Met | — |
| DoD-9 Ledger commit | Partial / not independently verified in R1 | Confirm tx |

## Findings

### [P3] `matters.schema_version` not updated by migrations (dual source of truth)

**Confidence:** High

**Requirement:** §3.3 schema versioning; DoD-2 migrations to current version

**Location:**
- `crates/matter-core/src/schema.rs` — `migrate()` updates only `schema_meta`
- `crates/matter-core/src/matter.rs` — `schema_version()` reads `schema_meta`; `info()` reads `matters.schema_version` set only at create

**Problem:** After a future schema migration, `Matter::schema_version()` and `MatterInfo.schema_version` can disagree.

**Correction:** On successful migration target version `N`, also `UPDATE matters SET schema_version = N` (or stop exposing denormalized field).

**Verification:** Integration test: simulate schema_meta advance / v2 migration, reopen, assert `info().schema_version == schema_version()`.

**Deferrable:** Yes at v1, but easy fix — fix in R1 loop.

## Completeness Sweep

No TODO/FIXME/todo!/unimplemented!/stub/fake success for required DoD.

## Completion Decision

**Verdict: PASS WITH DEFERRED P3** — fix P3 before final close for clean PASS.
