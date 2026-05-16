# ChangeGuard Command Reference

This document contains the full command catalog, flags, and category definitions for ChangeGuard.

## Core Commands

### Impact & Scan

```bash
changeguard scan --impact           # Before edits: full change intelligence
changeguard impact --all-parents    # Include side-branch commits in coupling analysis
changeguard impact --summary        # One-line triage: RISK | N changed | N couplings
changeguard impact --dead-code      # Include dead-code confidence analysis
```

### Verification

```bash
changeguard verify                         # Run configured or predicted verification
changeguard verify -c "cargo clippy -- -D warnings"   # Manual single command
changeguard verify --no-predict            # Skip predictive suggestions
```

### Reset

```bash
changeguard reset                          # Preserves config, rules, and ledger.db
changeguard reset --remove-config          # Remove .changeguard/config.toml
changeguard reset --remove-rules           # Remove .changeguard/rules.toml
changeguard reset --include-ledger --yes   # Destructive: wipe ledger.db
changeguard reset --all --yes              # Destructive: wipe the entire .changeguard tree
```

### Audit & Search

```bash
changeguard audit [--entity PATH] [--include-unaudited]  # Holistic provenance view
changeguard ledger audit [--entity PATH]                 # Same as above (legacy alias)
changeguard ledger search QUERY [--category CAT] [--days N] [--breaking] [--limit N] # FTS5 search
```

## Ledger Subcommands (Provenance)

```bash
changeguard ledger start PATH [--category CAT] [--message TEXT] [--issue REF]
changeguard ledger commit TX_ID --summary TEXT --reason TEXT [--change-type TYPE] [--breaking] [--auto-reconcile | --no-auto-reconcile]
changeguard ledger rollback TX_ID --reason TEXT
changeguard ledger atomic PATH --summary TEXT --reason TEXT [--category CAT]
changeguard ledger note PATH NOTE
changeguard ledger resume [ID]                              # Find most recent PENDING tx or resume specific    
changeguard ledger status [--entity PATH] [--compact]       # Holistic view or entity history
changeguard ledger reconcile [--tx-id ID] [--entity-pattern GLOB] [--all] --reason TEXT
changeguard ledger adopt [--tx-id ID] [--entity-pattern GLOB] [--all] [--reason TEXT]
changeguard ledger stack [--category CAT]                   # Show tech stack and validators
changeguard ledger register --rule-type TYPE --payload JSON [--force]   # Add enforcement rules
changeguard ledger adr [--output-dir DIR] [--days N]        # Export decisions to MADR
```

## Dead Code Detection

```bash
changeguard impact --dead-code                         # Include dead-code analysis in impact
changeguard dead-code [--threshold 0.75] [--limit 50]  # Full-repo proactive dead code scan
```

## Live Visualization (feature: viz-server)

```bash
changeguard viz-server [--port 8765] [--bind 127.0.0.1] [--open]   # Start WebSocket Arc Diagram server
changeguard viz-server --stop                                       # Stop a running viz server
```

## Watch

```bash
changeguard watch [--interval 1000] [--json]          # Watch repository for changes
changeguard watch --no-graph-sync                     # Disable live KG updates during watch
```

## Hotspots & Federation

```bash
changeguard hotspots --limit 20 --commits 500
changeguard hotspots --json
changeguard federate status
```

### Indexing & Search

```bash
changeguard index --docs              # Index markdown documentation
changeguard index --contracts         # Index OpenAPI/Swagger contracts
changeguard index --export-docs       # Export KG data to Markdown/Mermaid docs
changeguard index --export-docs --doc-type module_map --doc-type symbol_index  # Export specific doc types
changeguard index --all               # Full re-index
```

## Gemini-Assisted Reporting

```bash
changeguard ask "What should I verify next?"
changeguard ask --mode suggest "What checks should I run?"
changeguard ask --mode review-patch "Review the current diff."
changeguard ask --narrative
```

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
