# Ledgerful Command Reference

This document contains the full command catalog, flags, and category definitions for Ledgerful.

## Core Commands

### Impact & Scan

```bash
ledgerful scan --impact           # Before edits: full change intelligence
ledgerful impact --all-parents    # Include side-branch commits in coupling analysis
ledgerful impact --summary        # One-line triage: RISK | N changed | N couplings
ledgerful impact --dead-code      # Include dead-code confidence analysis
```

### Verification

```bash
ledgerful verify                         # Run configured or predicted verification (full scope)
ledgerful verify --scope fast            # Scoped: only tests covering changed files via test_mapping
ledgerful verify --scope full            # Full suite (default; CI should always use this)
ledgerful verify -c "cargo clippy -- -D warnings"   # Manual single command
ledgerful verify --no-predict            # Skip predictive suggestions
ledgerful verify --dry-run               # Show the plan without executing
```

**`--scope fast`** uses the `test_mapping` index to run only the test modules
that cover the changed files, emitting a nextest filterset command (e.g.
`cargo nextest run -E 'test(cli_scan) + test(dead_code_prune)'`). Falls back
to the full suite when shared infrastructure is touched (Cargo.toml,
cli/args.rs, config/**, migrations/**) or when `test_mapping` is empty.
The pre-push hook uses `--scope fast`. See `docs/testing.md` for the full
layered strategy.

### Reset

```bash
ledgerful reset                          # Preserves config, rules, and ledger.db
ledgerful reset --remove-config          # Remove .ledgerful/config.toml
ledgerful reset --remove-rules           # Remove .ledgerful/rules.toml
ledgerful reset --include-ledger --yes   # Destructive: wipe ledger.db
ledgerful reset --all --yes              # Destructive: wipe the entire .ledgerful tree
```

### Intent & Capture (Milestone O)

```bash
ledgerful intent demo                    # Launch the interactive intent capture TUI demo
ledgerful verify --signatures            # Mathematical verification of the entire ledger
```

### Audit & Search

```bash
ledgerful audit [--entity PATH] [--include-unaudited]  # Holistic provenance view
ledgerful ledger audit [--entity PATH]                 # Same as above (legacy alias)
ledgerful ledger search QUERY [--category CAT] [--days N] [--breaking] [--limit N] # FTS5 search
```

## Ledger Subcommands (Provenance)

```bash
ledgerful ledger start PATH [--category CAT] [--message TEXT] [--issue REF]
ledgerful ledger commit TX_ID --summary TEXT --reason TEXT [--change-type TYPE] [--breaking] [--auto-reconcile | --no-auto-reconcile]
ledgerful ledger rollback TX_ID --reason TEXT
ledgerful ledger atomic PATH --summary TEXT --reason TEXT [--category CAT]
ledgerful ledger status [--entity PATH] [--compact]       # Holistic view or entity history
ledgerful ledger reconcile [--tx-id ID] [--pattern GLOB] [--all] [--reason TEXT]
ledgerful ledger adopt [--pattern GLOB] [--all] --category CAT --summary TEXT --reason TEXT
ledgerful ledger stack [CAT]                              # Show tech stack and validators
ledgerful ledger register rule TERM --category CAT --reason REASON
ledgerful ledger register validator NAME --command CMD --category CAT [--timeout SEC]
ledgerful ledger adr [--output-dir DIR]                   # Export decisions to MADR
```

## Dead Code Detection

```bash
ledgerful impact --dead-code                         # Include dead-code analysis in impact
ledgerful dead-code [--threshold 0.75] [--limit 50]  # Full-repo proactive dead code scan
ledgerful dead-code --prune [--threshold 0.75]       # Interactively prune high-confidence dead code
```

`dead-code --prune` iterates through high-confidence findings and prompts
`[Y/n]` per symbol via `inquire`. Approved removals are written to disk and
documented in a `PENDING` ledger transaction with `DELETED` token provenance,
so tests must pass before `ledger commit` finalizes the deletion.

## Live Visualization (feature: viz-server)

```bash
ledgerful viz-server [--port 8765] [--bind 127.0.0.1] [--open]   # Start WebSocket Arc Diagram server
ledgerful viz-server --stop                                       # Stop a running viz server
```

## Watch

```bash
ledgerful watch [--interval 1000] [--json]          # Watch repository for changes
ledgerful watch --no-graph-sync                     # Disable live KG updates during watch
```

## Hotspots & Federation

```bash
ledgerful hotspots --limit 20 --commits 500
ledgerful hotspots --json
ledgerful federate status
```

### Indexing & Search

```bash
ledgerful index --docs              # Index markdown documentation
ledgerful index --contracts         # Index OpenAPI/Swagger contracts
ledgerful index --export-docs       # Export KG data to Markdown/Mermaid docs
ledgerful index --export-docs --doc-type module_map --doc-type symbol_index  # Export specific doc types
ledgerful index --full              # Full re-index
```

## Gemini-Assisted Reporting

```bash
ledgerful ask "What should I verify next?"
ledgerful ask --mode suggest "What checks should I run?"
ledgerful ask --mode review-patch "Review the current diff."
ledgerful ask --narrative
```

## Nightly Graph Indexing Scheduler

```bash
ledgerful schedule setup-nightly                # Install nightly `git fetch` + `index --analyze-graph`
ledgerful schedule setup-nightly --dry-run      # Print the generated scheduler syntax without registering it
ledgerful schedule setup-nightly --uninstall   # Remove the scheduled task
ledgerful schedule run-nightly                   # Run the sequence directly (git fetch, then index --analyze-graph)
```

- On **Windows** the command registers a `schtasks` daily task at 02:00 named `LedgerfulNightlyIndex`.
- On **macOS/Linux** it installs a crontab line at `0 2 * * *` that runs `ledgerful schedule run-nightly`.
- Output is appended to `.ledgerful/logs/nightly.log` with RFC3339 timestamps.

## Categories

| Category | Covers |
|---|---|
| `ARCHITECTURE` | High-level system design, multi-module contracts |
| `FEATURE` | New user-facing or internal functionality |
| `BUGFIX` | Defect repairs |
| `REFACTOR` | Structural improvement without behavior change |
| `INFRA` | CI, git hooks, Docker, build system |
| `TOOLING` | Internal scripts, dev tooling |
| `DOCS` | Documentation, README, ADRs |
| `CHORE` | Dependencies, formatting, minor cleanup |
