# PST-Dedupe

A pure Rust Windows tool for deduplicating emails across Outlook PST files.

## What It Does

- Opens one or more **Unicode PST files** (Outlook 2003+ format).
- Walks folders and extracts message properties.
- Detects duplicate emails with a tiered strategy:
  - **Tier 1:** `Message-ID` exact match (definitive).
  - **Tier 2:** SHA-256 content hash from subject, date, sender, body preview, and attachment metadata (fallback when Message-ID is missing).
- Produces a **CSV report** showing unique vs. duplicate messages.
- Optionally **exports unique messages as `.eml` files**.
- Presents the workflow through an **egui desktop app**.

## Build

Requires [Rust](https://rustup.rs/) 1.80+ on Windows.

```powershell
cargo build --release -p pst-dedup-gui
```

### Release Executable

The release binary is produced at:

```
target\release\pst-dedup-gui.exe
```

Approximate size: ~13 MB (self-contained, no external DLLs required).

To run:

```powershell
.\target\release\pst-dedup-gui.exe
```

## Test

```powershell
# Full workspace gate (format, clippy, tests)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Or use ChangeGuard:
changeguard verify
```

### PST Fixtures

Integration tests in `pst-reader` require a real Unicode PST file in `fixtures/`:

```powershell
# The fixtures/ directory is already gitignored.
# Place any .pst file there and run:
cargo test -p pst-reader --test integration
```

A small sample PST (271 KB) is used in CI/dev. See `conductor/track002-real-pst-fixtures-traversal/plan.md` for fixture sources.

## Architecture

| Crate | Responsibility |
|---|---|
| `pst-reader` | Pure Rust PST parser: header, NDB, LTP, messaging extraction |
| `dedup-engine` | Dedup hashing, index, CSV report, EML serialization |
| `pst-dedup-gui` | egui app and background scan worker |

See [`ARCHITECTURE.md`](ARCHITECTURE.md) for the MS-PST layer-by-layer implementation guide.

## Current Status

| Feature | Status |
|---|---|
| Unicode PST header parse | Works |
| NDB B-tree traversal | Works |
| LTP property/table read | Works |
| Folder/message traversal | Works (proven with real PST) |
| Tier 1 / Tier 2 dedup | Works, configurable |
| CSV report export | Works |
| EML export | Wired end-to-end |
| GUI scan progress | Works |
| Per-file error visibility | Works |
| ANSI PST support | Detected and rejected |
| CRC validation | Warning-only (algorithm under review) |
| Named property map | Stubbed (not needed for core dedup) |
| Mutex poisoning recovery | Works |
| UTF-8 safe truncation | Works |
| EML filename hardening | Works |
| Large-file stress testing | Pending |

## Verification Gate (Before Commit)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
changeguard verify
```

## License

MIT OR Apache-2.0
