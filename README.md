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
  - **`dedupe-desk`** — primary product shell: create/open matter, add sources, ingest/extract with live progress (track 0020)
  - **`pst-dedup` CLI** — agent- and script-friendly (`inspect`, `scan`, `dups`, `--json`, `--csv`)
  - **`pst-dedup-gui`** — legacy egui scan/dedup wizard (still builds for regression)

## Build

Requires [Rust](https://rustup.rs/) 1.80+ on Windows.

```powershell
# CLI (recommended for scripts and agents)
cargo build --release -p pst-dedup-cli

# Dedupe Desk (primary GUI)
cargo build --release -p dedupe-desk

# Legacy scan GUI
cargo build --release -p pst-dedup-gui
```

### Release Executables

| Binary | Path |
|---|---|
| Desk | `target\release\dedupe-desk.exe` |
| CLI | `target\release\pst-dedup.exe` |
| Legacy GUI | `target\release\pst-dedup-gui.exe` |

```powershell
.\target\release\dedupe-desk.exe
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

# Or use Ledgerful (same steps as .ledgerful/config.toml verify.steps):
ledgerful verify
```

## Git hooks + Ledgerful (Windows)

After clone, install hooks (requires [`ledgerful`](https://github.com/Ryan-AI-Studios/Ledgerful) on `PATH`):

```powershell
# PowerShell 7+ or Windows PowerShell 5.1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\install-hooks.ps1
# or: pwsh -File scripts\install-hooks.ps1
```

| Hook | What it runs |
|---|---|
| **pre-commit** | `ledgerful ledger status --compact --exit-code --verify-signatures` then `scripts\pre-commit.ps1` (fmt / clippy / test) |
| **pre-push** | Ledger status gate + `ledgerful verify --scope fast` |
| **commit-msg** / **post-commit** | Ledgerful intent sidecar + post-commit promotion |

Manual hygiene (same as pre-commit cargo steps):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\pre-commit.ps1
```

CI (GitHub Actions) runs **Windows-only** `fmt` / `clippy` / `test` on push and PRs (`.github/workflows/ci.yml`).

### PST Fixtures

Integration tests in `pst-reader` require Unicode PST files in `fixtures/`:

```powershell
cargo test -p pst-reader --test integration
```

Small Aspose/sample fixtures live under `fixtures/` (see `fixtures/README.md`). Real multi-mailbox PSTs are useful for **manual** CLI/Desk smoke on a **local path only** — never commit case evidence. Implementation track docs live under local `conductor/` (gitignored; not published with this repo).

## Architecture

| Crate | Responsibility |
|---|---|
| `pst-reader` | Pure Rust PST parser: header, NDB, LTP, messaging extraction |
| `dedup-engine` | Dedup hashing, index, CSV report, EML serialization |
| `pst-dedup-cli` | CLI surface: inspect / scan / dups (JSON + CSV) |
| `pst-dedup-gui` | egui app and background scan worker |
| `pst-writer` | Experimental/fixture PST writing and EML import helpers |
| `matter-core` | Matter layout + SQLite (schema v9: Normalized Item + dedupe/thread/neardup/cull/promote + `review_sets` + coding + `saved_searches` + review-list index + metadata filters) + CAS (`put_bytes` / streaming `put_reader`) + audit + jobs + logical_hash v1 + `workspace/temp/` |
| `ingest-purview` | Purview/package/ZIP detect + safe expand + resumable inventory (blocking worker API; `*_on_job` for runner) |
| `extract-pst` | PST → Normalized Items + families + logical_hash; `pst-native-message-v1` native (not EML); mid-folder resume (blocking; `*_on_job` for runner) |
| `process-runner` | In-process job runner: single matter worker, cancel, watch progress, Option C job-id authority |
| `matter-cull` | Flag-only data reduction: built-in + user presets, family fixpoint, `cull_*` result columns (never deletes items/CAS) |
| `matter-promote` | Flag-only promote-to-review: policies + bidirectional family expand + single-pass `review_order` (never deletes items/CAS) |

**Matter layout** (Desk foundation): `matter.db`, `blobs/sha256/<aa>/<hex>`, reserved `index/` / `exports/` / `logs/`, `workspace/temp/`.  
See [`crates/matter-core/README.md`](crates/matter-core/README.md), [`crates/ingest-purview/README.md`](crates/ingest-purview/README.md), [`crates/extract-pst/README.md`](crates/extract-pst/README.md), [`crates/process-runner/README.md`](crates/process-runner/README.md), and [`ARCHITECTURE.md`](ARCHITECTURE.md).

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
