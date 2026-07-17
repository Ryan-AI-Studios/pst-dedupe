---
name: onboarding
description: Trigger when starting a new session on this repo, when an agent needs orientation, or when asked "onboard me", "where do I start?", "what's the project state?", or "how does work get done here?".
---

# PST-Dedupe Onboarding

You are working on **pst-dedupe**: a Rust workspace for a standalone Windows PST email deduplication tool.

## Purpose

Build a pure Rust PST deduper that:

- Opens one or more Unicode PST files (including Permute-encrypted stores).
- Reads folders, messages, and attachment metadata from PST internals without Outlook or libpff.
- Detects duplicate emails with a tiered strategy:
  - Tier 1: normalized `PidTagInternetMessageId`.
  - Tier 2: content hash from subject, submit time, sender, body preview, and optional attachment metadata.
- Produces a CSV report.
- Optionally exports unique messages as EML (GUI path).
- Surfaces: **CLI** (`pst-dedup`) and **egui** desktop app.

## Repository Map

| Path | Responsibility |
|---|---|
| `ARCHITECTURE.md` | Design blueprint and PST implementation notes. |
| `README.md` | User-facing build, CLI, and status. |
| `crates/pst-reader` | Pure Rust PST reader: header, NDB, LTP, messaging extraction. |
| `crates/dedup-engine` | Dedup hashing, index, CSV report, EML serialization. |
| `crates/pst-dedup-cli` | CLI binary `pst-dedup`: inspect / scan / dups. |
| `crates/pst-dedup-gui` | egui app and background scan worker. |
| `crates/pst-writer` | Experimental PST writing / fixture helpers. |
| `conductor/conductor.md` | Track board and implementation state. |
| `.agents/rules` | Repo-specific AI workflow constraints. |
| `.agents/skills/coding-core/SKILL.md` | Core coding standards and patterns. |
| `.agents/skills/ledgerful/SKILL.md` | Ledgerful usage guidelines. |
| `.agents/skills/ai-brains/SKILL.md` | ai-brains usage guidelines. |
| `.agents/skills/orchestrator-workflow/SKILL.md` | Orchestrator workflow guidelines. |
| `.agents/skills/tooling/SKILL.md` | Tooling usage guidelines. |

## Current Reality Check

At onboarding time, agents should assume:

- Prefer **CLI** for agent-driven scans: `target\release\pst-dedup.exe` or `cargo run -p pst-dedup-cli -- …`.
- Reader correctness has been proven on fixtures **and** a real multi-mailbox Permute PST (header crypt alignment, HN page map, TC RowIndex).
- GUI remains interactive; agents should not depend on driving egui.
- Large-file stress testing and full CRC algorithm verification remain open.
- Always verify current compile/test state locally — the project moves quickly.

## Default First Commands

```powershell
git status --short --branch
cargo test -p dedup-engine
cargo test -p pst-reader
cargo check -p pst-dedup-cli
cargo check -p pst-dedup-gui
ledgerful doctor
ledgerful ledger status
ai-brains context --show
```

Smoke a real PST with the CLI when available:

```powershell
cargo run -p pst-dedup-cli --release -- inspect path\to\file.pst --top 10
cargo run -p pst-dedup-cli --release -- scan path\to\file.pst --json
```

## Engineering Priorities

1. Keep the workspace compiling (all crates).
2. Prefer fixture + real-PST proof for reader correctness.
3. Fix correctness bugs before UI polish.
4. Keep PST parsing conservative and spec-grounded.
5. Use Ledgerful for risk, impact, and provenance.
6. Use ai-brains for project memory, decisions, and safety signals.
7. Prefer CLI for automated verification; GUI for human workflow.

## Verification Gate

Before any commit or push, run the narrowest relevant checks plus the full gate when feasible:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

If a command cannot run, record the exact blocker in the final note or ledger.
