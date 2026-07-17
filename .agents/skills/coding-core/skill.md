---
name: coding-core
description: Use when writing, modifying, or reviewing Rust code in pst-dedupe, especially PST parsing, dedup semantics, worker behavior, CLI, or GUI state.
---

# Coding Core - pst-dedupe

## Crate Boundaries

| Crate | Responsibility |
|---|---|
| `pst-reader` | Read-only PST parser and message extraction. No dedup policy. |
| `dedup-engine` | Message identity policy, hash/index/report/export logic. No PST parsing. |
| `pst-dedup-cli` | CLI orchestration: inspect, scan, dups; JSON/CSV output. Composes reader + engine. No GUI. |
| `pst-dedup-gui` | User workflow, background worker, progress/results UI. No low-level PST parsing. |
| `pst-writer` | Experimental PST writing and fixture/EML import helpers. Keep separate from read-only product path. |
| `matter-core` | Matter layout, SQLite metadata, physical CAS, audit chain, jobs/checkpoints. No ZIP/PST I/O. |
| `ingest-purview` | Package detect + safe ZIP expand into matter. Blocking-worker API only; no PST message extract. |

## Rust Standards

- Use `Result<T, E>` for fallible operations.
- Keep parser errors typed and actionable.
- Avoid `unwrap`, `expect`, and panic paths in production code.
- Avoid silent fallback that hides data loss.
- Prefer byte-level tests for binary parsing.
- Keep dependencies permissive and minimal.

## PST Reader Rules

- Parse little-endian structures deliberately.
- Validate magic/version/sentinel/trailers where practical.
- Unicode header: `rgbFM`/`rgbFP` are 128+128; `bCryptMethod` is at offset `0x201`; ROOT is 72 bytes with `fAMapValid` + `bReserved` + `wReserved`.
- HNPAGEMAP is `cAlloc` + `cFree` + `rgibAlloc[]` (do not skip `cFree`).
- Hierarchy/contents child NIDs come from the TC **RowIndex BTH** (RowID), with `PidTagLtpRowId` only as fallback.
- Treat malformed or unsupported PST features as errors with context.
- Do not claim ANSI PST support unless implemented and tested.
- Do not claim encrypted PST support unless the relevant mode passes fixture or real-PST tests (Permute is proven; keep Cyclic unit-tested).
- Do not skip folder/table errors and report success unless best-effort mode exists.

## Dedup Rules

- Tier 1: normalized Message-ID.
- Tier 2: content hash fallback for missing Message-ID.
- If Tier 2 can be disabled, disabled means no content-hash lookup or insertion.
- Preserve first-seen original message as the duplicate target.
- Tests should cover distinct Message-IDs with matching content, missing Message-ID duplicates, and Tier 2 disabled behavior.

## CLI Rules

- Prefer structured output (`--json`) for agent consumption; logs go to stderr.
- Non-zero exit on hard failures (missing path, failed files after scan).
- Do not print secrets or full mail bodies by default; subjects/folders may still be sensitive — keep logs scoped.
- Keep scan orchestration in `pst-dedup-cli` (or shared modules), not inside `pst-reader`.

## GUI Rules

- Keep the UI honest about scan failures.
- Worker cancellation should stop promptly and mark results as partial.
- Export buttons should perform real work or be disabled/clearly unavailable.
- Avoid blocking the egui thread with PST I/O.

## Verification Bias

- PST-reader edits: `cargo test -p pst-reader` plus fixture tests; real-PST CLI smoke when available.
- Dedup edits: `cargo test -p dedup-engine`.
- CLI edits: `cargo build -p pst-dedup-cli` and a `--json` scan/inspect smoke.
- GUI edits: `cargo check -p pst-dedup-gui` and relevant manual run when possible.
