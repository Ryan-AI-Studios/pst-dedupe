---
name: orchestrator-workflow
description: Defines the standard operating procedure for planning tracks, using ChangeGuard, using ai-brains, running verification, and coordinating reviews for pst-dedupe.
---

# Orchestrator Workflow

Use this workflow for non-trivial changes in `pst-dedupe`.

## Track System

Tracks live under `conductor/` and are summarized in `conductor/conductor.md`.

A track is a bounded unit of work with:

- Objective.
- Affected crates/files.
- Risks and assumptions.
- Verification plan.
- Completion notes.

Suggested categories:

- `READER`: PST header, NDB, LTP, messaging, fixtures.
- `DEDUP`: hashing, index semantics, reports, EML export.
- `GUI`: egui app state, worker progress, user workflows.
- `INFRA`: Cargo, CI, hooks, release packaging.
- `DOCS`: architecture, operator notes.
- `BUGFIX`: correctness fixes.

## Session Start

Run:

```powershell
ai-brains context --show
ai-brains safety sync
ai-brains preflight --max-words 1000
changeguard ledger status
changeguard hotspots --limit 5
git status --short --branch
```

If ChangeGuard or ai-brains is unavailable, continue with local inspection and report that tool signal as unavailable.

## Planning

For meaningful work:

1. Read `ARCHITECTURE.md` and relevant crate files.
2. Check `conductor/conductor.md`.
3. Run `changeguard scan --impact`.
4. Start a ledger transaction:

```powershell
changeguard ledger start <entity> --category <CAT> --message "Intent"
```

5. Record durable constraints or decisions:

```powershell
ai-brains pin "DECISION: ..."
```

Use `--role user` when pinning a direct user correction or instruction.

## Implementation

Keep changes scoped to the track. Prefer:

- Small Rust modules with explicit error types.
- `Result<T, PstError>` or crate-local errors for fallible parsing.
- Spec-backed parsing over guesswork.
- Tests that exercise real bytes, preferably real or minimal synthetic PST fixtures.
- Clear separation between PST reading and dedup policy.

Do not claim support for large PSTs, Unicode PSTs, encryption modes, attachments, or EML export unless tests demonstrate it.

## Verification

Run targeted checks first, then the full gate:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
changeguard impact
changeguard verify
```

For PST-reader changes, add or run fixture/integration tests. For dedup semantics, add unit tests that distinguish Message-ID and content-hash behavior.

## Review

For high-risk reader changes, run a read-only review:

```powershell
codex exec -C "." -s read-only -m gpt-5.4 -o review.md "Review the current git diff for critical/high correctness issues in PST parsing and dedup behavior. Do not modify files."
```

Critical/high review findings must be fixed or explicitly tracked before completing the track.

## Finalization

Before commit:

1. Update `conductor/conductor.md`.
2. Run `changeguard impact`.
3. Run verification.
4. Close the ledger transaction:

```powershell
changeguard ledger commit <tx-id> --summary "What changed" --reason "Why"
```

Then commit normally.
