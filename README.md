# PST-Dedupe

A pure Rust Windows tool for deduplicating emails across Outlook PST files.

## What It Does

- Opens one or more **Unicode PST files** (Outlook 2003+ format), including **Permute**-encrypted stores.
- Walks folders and extracts message properties.
- Detects duplicate emails with a tiered strategy:
  - **Tier 1:** `Message-ID` exact match (definitive).
  - **Tier 2:** SHA-256 content hash from subject, date, sender, body preview, and attachment metadata (fallback when Message-ID is missing).
- Produces a **CSV report** showing unique vs. duplicate messages.
- Optionally **exports unique messages as `.eml` files** (GUI path).
- Surfaces:
  - **`pst-dedup` CLI** — agent- and script-friendly (`inspect`, `scan`, `dups`, `--json`, `--csv`)
  - **egui desktop app** — interactive file pick / progress / results

## Build

Requires [Rust](https://rustup.rs/) 1.80+ on Windows.

```powershell
# CLI (recommended for scripts and agents)
cargo build --release -p pst-dedup-cli

# GUI
cargo build --release -p pst-dedup-gui
```

### Release Executables

| Binary | Path |
|---|---|
| CLI | `target\release\pst-dedup.exe` |
| GUI | `target\release\pst-dedup-gui.exe` |

```powershell
.\target\release\pst-dedup.exe --help
.\target\release\pst-dedup-gui.exe
```

## CLI Usage

```powershell
# Structure + folder counts
.\target\release\pst-dedup.exe inspect archive.pst --top 20

# Full dedup summary (machine-readable)
.\target\release\pst-dedup.exe scan archive.pst --json

# Duplicates only
.\target\release\pst-dedup.exe dups archive.pst --limit 25 --json

# CSV report (+ summary footer)
.\target\release\pst-dedup.exe scan archive.pst --csv output\report.csv

# Multiple PSTs
.\target\release\pst-dedup.exe scan a.pst b.pst --json --dups --limit 50
```

Useful flags: `--no-tier2`, `--no-attachments`, `-v` / `-vv` (logs on stderr).  
For quiet agent runs: `$env:RUST_LOG = 'error'`.

## Test

```powershell
# Full workspace gate (format, clippy, tests)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Or use Ledgerful:
ledgerful verify
```

### PST Fixtures

Integration tests in `pst-reader` require Unicode PST files in `fixtures/`:

```powershell
cargo test -p pst-reader --test integration
```

Small Aspose/sample fixtures live under `fixtures/`. Real multi-mailbox PSTs are useful for manual CLI smoke tests; keep sensitive mail out of git and public logs.

## Architecture

| Crate | Responsibility |
|---|---|
| `pst-reader` | Pure Rust PST parser: header, NDB, LTP, messaging extraction |
| `dedup-engine` | Dedup hashing, index, CSV report, EML serialization |
| `pst-dedup-cli` | CLI surface: inspect / scan / dups (JSON + CSV) |
| `pst-dedup-gui` | egui app and background scan worker |
| `pst-writer` | Experimental/fixture PST writing and EML import helpers |
| `matter-core` | Matter layout + SQLite + CAS + audit hash chain + jobs/checkpoints |

**Matter layout** (Desk foundation): `matter.db`, `blobs/sha256/<aa>/<hex>`, reserved `index/` / `exports/` / `logs/`.  
See [`crates/matter-core/README.md`](crates/matter-core/README.md) and [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Current Status

| Feature | Status |
|---|---|
| Unicode PST header parse | Works (including correct `bCryptMethod` alignment) |
| NDB B-tree traversal | Works |
| LTP HN / BTH / TC | Works (HNPAGEMAP `cFree`, TC RowIndex NIDs) |
| Folder/message traversal | Works (fixtures + real multi-mailbox PST) |
| NDB_CRYPT_PERMUTE | Works (verified on encrypted real PST) |
| NDB_CRYPT_CYCLIC | Implemented with unit tests |
| Tier 1 / Tier 2 dedup | Works, configurable |
| CSV report export | Works (CLI + engine) |
| CLI inspect / scan / dups | Works (`--json`, `--csv`) |
| EML export | Wired end-to-end in GUI |
| GUI scan progress | Works |
| Per-file error visibility | Works |
| ANSI PST support | Detected and rejected |
| CRC validation | Warning-only (algorithm under review) |
| Named property map | Stubbed (not needed for core dedup) |
| Large-file stress testing | Pending |

## Verification Gate (Before Commit)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

## License

MIT OR Apache-2.0
