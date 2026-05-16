---
name: coding-core
description: Use when writing, modifying, or reviewing Rust code in pst-dedupe, especially PST parsing, dedup semantics, worker behavior, or GUI state.
---

# Coding Core - pst-dedupe

## Crate Boundaries

| Crate | Responsibility |
|---|---|
| `pst-reader` | Read-only PST parser and message extraction. No dedup policy. |
| `dedup-engine` | Message identity policy, hash/index/report/export logic. No PST parsing. |
| `pst-dedup-gui` | User workflow, background worker, progress/results UI. No low-level PST parsing. |

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
- Treat malformed or unsupported PST features as errors with context.
- Do not claim ANSI PST support unless implemented and tested.
- Do not claim encrypted PST support unless the relevant mode passes fixture tests.
- Do not skip folder/table errors and report success unless best-effort mode exists.

## Dedup Rules

- Tier 1: normalized Message-ID.
- Tier 2: content hash fallback for missing Message-ID.
- If Tier 2 can be disabled, disabled means no content-hash lookup or insertion.
- Preserve first-seen original message as the duplicate target.
- Tests should cover distinct Message-IDs with matching content, missing Message-ID duplicates, and Tier 2 disabled behavior.

## GUI Rules

- Keep the UI honest about scan failures.
- Worker cancellation should stop promptly and mark results as partial.
- Export buttons should perform real work or be disabled/clearly unavailable.
- Avoid blocking the egui thread with PST I/O.

## Verification Bias

- PST-reader edits: `cargo test -p pst-reader` plus fixture tests.
- Dedup edits: `cargo test -p dedup-engine`.
- GUI edits: `cargo check -p pst-dedup-gui` and relevant manual run when possible.
