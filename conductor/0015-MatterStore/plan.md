# 0015 — Matter store + audit log + blob CAS — Plan

Phased checklist. Map phases to DoD items in `spec.md` §7. Execute in `C:\dev\dedupe`.

> **Ledger:** open before implementation —  
> `ledgerful ledger start 0015-matterstore --category ARCHITECTURE --message "matter-core: SQLite store, CAS, audit chain, jobs/checkpoints"`  
> — commit in Finalize.

---

## Phase 0 — Session baseline + preconditions → DoD-7 prep

- [ ] Onboarding baseline (Agents.md / onboarding skill):
  - [ ] `ledgerful doctor`
  - [ ] `ledgerful ledger status --compact`
  - [ ] `ai-brains context --show` (if available; if not, note in review.md)
- [ ] Read `C:\dev\Dedupe-plan.md` §§2.3, 4.x, 5.2, 5.5, 17 (deps)
- [ ] Read `../TRACK-GUARDRAILS.md`
- [ ] `cargo check --workspace` green (or record blockers)
- [ ] Open ledger transaction (see header)

## Phase 1 — Crate + schema design → DoD-1 prep

- [ ] `cargo new --lib crates/matter-core --name matter-core`
- [ ] Add `matter-core` to workspace `members` in root `Cargo.toml`
- [ ] Add workspace-friendly deps: `rusqlite` (`bundled`), `sha2`, `thiserror` 2, `serde`/`serde_json`, `camino`, `tempfile` (dev)
- [ ] Write short `crates/matter-core/README.md` (layout + CAS + audit contract)
- [ ] Freeze schema DDL in code (versioned migration list):
  - matters, sources, items (`native_sha256`, `logical_hash` nullable), item_families
  - **item_errors**
  - **jobs**, **job_checkpoints**
  - **audit_events** with `prev_hash` / `entry_hash`
- [ ] Document CAS path layout + SHA-256 physical-only rule (spec §3.4)

## Phase 2 — Implementation → DoD-1…DoD-6

- [ ] `Matter::create` / `Matter::open` (path, name)
- [ ] Migrations apply on open
- [ ] CAS: `put_bytes` / `get_bytes` / `exists` (SHA-256, no clobber)
- [ ] Jobs API: create, set state, put/get checkpoint
- [ ] `item_errors::record` + query by source/job
- [ ] Audit: `append_event` + `verify_audit_chain`
- [ ] Unit/integration tests under `matter-core` (tempdir matters)
- [ ] No UI required; optional thin CLI later tracks can wrap this

## Phase 3 — Verification gate → DoD-7

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test -p matter-core`
- [ ] `cargo test --workspace`
- [ ] **`ledgerful verify`** (record output; if blocked, exact command + fallback per fail policy)
- [ ] Capture evidence snippets for `review.md`

## Phase 4 — Finalize → DoD-8, DoD-9

- [ ] Update `ARCHITECTURE.md` (crate map: `matter-core`) and/or `README.md` architecture table
- [ ] Write `review.md` (results, schema version, deferred items)
- [ ] Update `../conductor.md`: **0015** status → **Completed**
- [ ] Update `../sequencing.md` if needed
- [ ] Commit ledger transaction
- [ ] Notify unblocked: **0016, 0017, 0019** (and indirectly 0018)

---

## Handoff notes

- **Downstream:** 0016 (sources + zip checkpoints), 0017 (fill logical_hash / families), 0018 (PST → items), 0019 (job runner uses jobs/checkpoints).
- **Do not** store normalized/logical payloads in CAS — only raw physical bytes.
- **Do not** implement Purview/PST I/O here.
- Prefer small public API surface; keep SQL private to the crate.
- Single-exe / no-daemon invariant unchanged (this crate is library-only).
