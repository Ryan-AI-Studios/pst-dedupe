---
name: tooling
description: Use when searching the repo, running Cargo checks, using Ledgerful, using ai-brains, GitHub CLI, or preparing verification.
---

# Tooling - pst-dedupe

## Search

Use:

```powershell
rg --files
rg -n "pattern" crates ARCHITECTURE.md .agents conductor
```

## Rust

Primary commands:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check -p pst-dedup-cli
cargo check -p pst-dedup-gui
cargo test -p pst-reader
cargo test -p dedup-engine
```

CLI smoke (agent-friendly):

```powershell
cargo run -p pst-dedup-cli --release -- inspect path\to\file.pst --json
cargo run -p pst-dedup-cli --release -- scan path\to\file.pst --json
# Binary after release build:
# .\target\release\pst-dedup.exe scan path\to\file.pst --csv output\report.csv
```

## Ledgerful

Use Ledgerful for repo intelligence and provenance:

```powershell
ledgerful doctor
ledgerful scan --impact
ledgerful hotspots --limit 10
ledgerful impact
ledgerful verify
ledgerful ledger status
```

Do not edit `.ledgerful/` state directly.

## ai-brains

Use ai-brains for persistent project context:

```powershell
ai-brains context --show
ai-brains safety sync
ai-brains preflight --max-words 1000
ai-brains recall "pst-dedupe"
ai-brains pin "CONSTRAINT: ..."
```

## GitHub

Remote:

```powershell
git remote add origin https://github.com/UnlikelyKiller/pst-dedupe.git
git branch -M main
git push -u origin main
```

Before pushing, make sure the Cargo gate and `ledgerful verify` have been run or the reason they cannot run is documented.
