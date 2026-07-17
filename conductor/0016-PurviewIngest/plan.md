# 0016 — Purview package ingest + ZIP safety — Plan

Phased checklist. Map phases to DoD items in `spec.md` §7. Execute in `C:\dev\dedupe`.

> **Ledger:**  
> `ledgerful ledger start 0016-purviewingest --category FEATURE --message "Purview package ingest + ZIP safety"`  
> Commit in Finalize. Use `SECURITY` category only if the ledger entry is primarily about harden/fuzz surface.

---

## Phase 0 — Preconditions → DoD-8 baseline

- [x] Confirm **0015** completed: `crates/matter-core` exists; read `../0015-MatterStore/review.md` + `crates/matter-core/README.md`
- [x] Read plan-of-record: `C:\dev\Dedupe-plan.md` §§2.1–2.2, 4.6, 5.1, 5.5, Series A **016**, §17 zip pin
- [x] Note plan §4.6 top-level grain is a **floor**; this track implements **stricter leaf/sub-entry** checkpoints (`spec.md` §3.5)
- [x] `cargo test -p matter-core` green
- [x] Note matter-core gaps: `update_source`, lookup inventory by `(source_id, path)` — Phase 1
- [x] Inventory fixtures strategy: **synthetic only** under `fixtures/purview/` (no client mail in git)

## Phase 1 — Design / API / matter-core glue → DoD-1 prep, DoD-4

- [x] Finalize public API names (`detect`, `ingest_path`, `resume_ingest`, `ExpandLimits`, `PackageKind`)
- [x] Define stable kind strings stored on `sources.kind`:
  - `single_pst` | `single_zip` | `purview_package` | `raw_dump` | `unsupported`
- [x] Define job kind: `ingest`; checkpoint stage: `expand`
- [x] Define audit action names: `ingest.start`, `ingest.source`, `ingest.complete`, `ingest.fail`
- [x] Define item statuses for inventory: `discovered` | `expanded` | `error`
- [x] Define error codes list (`zip_path_traversal`, `zip_absolute_path`, `zip_bomb_size`, `zip_bomb_ratio`, `zip_bomb_entries`, `zip_depth`, `zip_corrupt`, `unsupported_7z`, `cancelled`, `io_error`, …)
- [x] Design `cursor_json` for **mega-zip resume**:
  - [x] `last_successfully_extracted_logical_path`
  - [x] `completed_count`, `bytes_extracted`
  - [x] optional `completed_top_level` / `archive_stack`
  - [x] cadence fields: `checkpoint_every_n_entries`, `checkpoint_every_bytes`
- [x] Add **minimal** matter-core APIs if missing:
  - [x] `Matter::update_source(id, status, cursor_json)`
  - [x] `item_exists_for_source_path(source_id, path)` or `list_items_by_source` sufficient for skip checks
  - [x] Optional: `list_sources`
  - [x] Keep schema at **v1** unless a unique index on `(source_id, path)` is justified (nice-to-have)
- [x] Document limit defaults + **blocking-thread caller contract** in crate README draft

## Phase 2 — Scaffold crate → DoD-1

- [x] `cargo new --lib crates/ingest-purview --name ingest-purview`
- [x] Add workspace member + path dep on `matter-core`
- [x] Pin `zip = "8.6"` with **trimmed features** (start with `deflate` only; expand if fixtures need more)
- [x] Modules sketch:
  - `detect.rs` — package kind
  - `path_safety.rs` — decode names + normalize/reject
  - `encoding.rs` — UTF-8 / CP437 / Win-1252 fallbacks (or fold into path_safety)
  - `expand.rs` — ZIP walk + limits + nested zip + leaf checkpoints
  - `ingest.rs` — matter wiring, jobs, checkpoints, audit
  - `limits.rs` — `ExpandLimits` defaults including checkpoint cadence
  - `error.rs` — typed errors
  - `lib.rs` — re-exports; module docs warn about blocking use
- [x] Skeleton compiles: `cargo check -p ingest-purview`

## Phase 3 — Path safety + encoding + detector → DoD-2, DoD-3 (partial), DoD-7 (partial)

- [x] Implement entry-name pipeline:
  - [x] Decode: UTF-8 flag / valid UTF-8 → else CP437 → else Win-1252/Latin-1 (spec §3.3.1)
  - [x] Reject `..`, empty, absolute Win/Unix paths, reserved device names if relevant
  - [x] Normalize to package-relative UTF-8 logical path (slash style internal)
- [x] Unit tests: sanitizer edge cases + **at least one non-UTF-8 name** that decodes and is accepted
- [x] Implement `detect(path)` heuristics + tests (temp dirs/files)
- [x] Property tests **or** fuzz target on sanitizer inputs (no panic; always reject traversal after decode)

## Phase 4 — ZIP expand + CAS inventory → DoD-3, DoD-4

- [x] Expand single ZIP → stream entries → `matter.put_bytes` → `insert_item` inventory
- [x] Nested ZIP recursion with depth cap
- [x] Register discovered `.pst` paths (inventory + optional child source rows) **without** pst-reader
- [x] Enforce limits (size / ratio / entry count); fail closed with codes
- [x] Reject unsafe names before any write
- [x] `.7z` → record unsupported (error path), do not expand
- [x] Integration test: synthetic nested zip fixture → CAS digests match file bytes
- [x] Integration test: legacy-encoded entry name → inventory path UTF-8 + CAS ok

## Phase 5 — Job / checkpoint / resume / audit → DoD-5, DoD-6

- [x] Wire `ingest_path`: insert source, create job, Running, expand, Succeeded/Failed
- [x] Checkpoint stage `expand` on **leaf** success cadence (`checkpoint_every_n_entries` / `checkpoint_every_bytes`), always updating `last_successfully_extracted_logical_path`
- [x] Dual-update source status + cursor via `update_source` (optional mirror; job checkpoint is SoT)
- [x] `resume_ingest`: load checkpoint; **skip** any leaf already in inventory for `(source_id, path)` with digest (authoritative)
- [x] Integration test **mega-zip grain**: multi-entry zip; interrupt after entry *k* of *n* (inner leaves, not “whole top-level zip done”) → resume does not re-`put_bytes` for 1..k
- [x] Optional secondary test: two top-level members still work
- [x] Audit: start + complete/fail; `verify_audit_chain` passes
- [x] Partial failure: bad entry records `item_errors`, good entries kept (honest partial)

## Phase 6 — Fixtures + docs → DoD-7, DoD-9 prep

- [x] Add `fixtures/purview/` synthetic package:
  - top-level dummy `mail.pst` (empty/minimal bytes or copy tiny fixture header-only if safe)
  - `files.zip` containing nested `inner.zip` + text file
  - optional “report.csv” noise for purview heuristic
- [x] Malicious fixtures generated in-test (do not need committed bombs if generated)
- [x] `crates/ingest-purview/README.md` **must** include:
  - [x] Blocking-thread / `spawn_blocking` / rayon warning for 0019+ and GUI
  - [x] Resume grain (leaf + cadence)
  - [x] Encoding fallback policy
  - [x] Default limits
- [x] Touch root `ARCHITECTURE.md` + `README.md` crate tables
- [x] Optional: CLI smoke command (not required for DoD)

## Phase 7 — Verification → DoD-7, DoD-8

- [x] `cargo test -p ingest-purview`
- [x] `cargo test -p matter-core` (regressions from update_source / lookups)
- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `ledgerful verify` (**required** — not optional)
- [x] Capture counts/commands for `review.md`

## Phase 8 — Finalize → DoD-9

- [x] Write `review.md` (API summary, limits defaults, resume grain, encoding policy, test evidence, deferred 7z/PST parse)
- [x] Update `../conductor.md`: **0016** → **Completed**
- [x] Update `../sequencing.md` status markers if used
- [x] Commit ledger transaction with summary/reason
- [x] Note unblocked: **0018** (with **0017**), downstream package path for Desk shell; remind 0019 of blocking-pool contract

---

## Suggested module / file map

```
crates/ingest-purview/
  Cargo.toml
  README.md                 # includes blocking-thread WARNING
  src/
    lib.rs
    error.rs
    limits.rs
    detect.rs
    encoding.rs             # optional module
    path_safety.rs
    expand.rs
    ingest.rs
  tests/
    integration.rs
fixtures/purview/           # synthetic only
  sample_package/
    mail.pst
    files.zip
    ExportSummary.csv
```

---

## Default limits (starting point — tune in review)

| Limit | Default | Test override |
|---|---|---|
| `max_uncompressed_bytes` | 50 * 1024^3 (50 GiB) | small (e.g. 1 MiB) |
| `max_compression_ratio` | 100.0 | 2.0 |
| `max_entries` | 500_000 | 20 |
| `max_zip_depth` | 8 | 2–3 |
| `checkpoint_every_n_entries` | 50 | 1 |
| `checkpoint_every_bytes` | 64 * 1024 * 1024 (64 MiB) | small (e.g. 1 KiB) |

Document final defaults in crate README.

---

## Handoff notes

- **Do not** open PSTs with `pst-reader` here — only discover/register.
- **Do not** write into the user’s export directory.
- **Do not** compute `logical_hash` (0017/0018).
- ZIP logic stays in `ingest-purview`; matter-core remains storage-only.
- **Resume is inventory-authoritative:** skip CAS when `(source_id, path)` already has `native_sha256`.
- **0019** must call ingest APIs from a blocking pool; document that in both READMEs if helpful.
- Downstream **0018** expects inventory items with `path` + `native_sha256` pointing at CAS for containerized files, and discoverable `.pst` paths for extraction.
- If real Purview multi-part naming (`export.zip`, `export1.zip`, …) is observed later, extend detector heuristics without breaking kind strings.
- Single-exe / no-daemon invariant unchanged.
