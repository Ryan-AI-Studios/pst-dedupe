---
name: onboarding
description: Trigger when starting a new session on this repo, when an agent needs orientation, or when asked "onboard me", "where do I start?", "what's the project state?", or "how does work get done here?".
---

# PST-Dedupe Onboarding

You are working on **pst-dedupe**: a Rust workspace intended to build a standalone Windows PST email deduplication tool.

## Purpose

Build a pure Rust PST deduper that:

- Opens one or more Unicode PST files.
- Reads folders, messages, and attachment metadata from PST internals without Outlook or libpff.
- Detects duplicate emails with a tiered strategy:
  - Tier 1: normalized `PidTagInternetMessageId`.
  - Tier 2: content hash from subject, submit time, sender, body preview, and optional attachment metadata.
- Produces a CSV report.
- Optionally exports unique messages as EML.
- Presents the workflow through an egui desktop app.

## Repository Map

| Path | Responsibility |
|---|---|
| `ARCHITECTURE.md` | Design blueprint and PST implementation notes. |
| `crates/pst-reader` | Pure Rust PST reader: header, NDB, LTP, messaging extraction. |
| `crates/dedup-engine` | Dedup hashing, index, CSV report, EML serialization. |
| `crates/pst-dedup-gui` | egui app and background scan worker. |
| `conductor/conductor.md` | Track board and implementation state. |
| `.agents/rules` | Repo-specific AI workflow constraints. |

## Current Reality Check

This repo is not complete yet. At onboarding time, agents should assume:

- The architecture doc is a blueprint, not proof of implementation.
- The GUI crate may not compile until current build issues are fixed.
- `pst-reader` lacks real PST fixture/integration coverage.
- Real PST parsing correctness must be proven with fixtures before product claims are made.

Always verify the current state locally because this project is moving quickly.

## Default First Commands

```powershell
git status --short --branch
cargo test -p dedup-engine
cargo test -p pst-reader
cargo check -p pst-dedup-gui
changeguard doctor
changeguard ledger status
ai-brains context --show
```

## Engineering Priorities

1. Make the workspace compile.
2. Add real PST fixture coverage for the reader.
3. Fix correctness bugs before UI polish.
4. Keep PST parsing logic conservative and spec-grounded.
5. Use ChangeGuard for risk, impact, and provenance.
6. Use ai-brains for project memory, decisions, and safety signals.

## Verification Gate

Before any commit or push, run the narrowest relevant checks plus the full gate when feasible:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
changeguard verify
```

If a command cannot run, record the exact blocker in the final note or ledger.
