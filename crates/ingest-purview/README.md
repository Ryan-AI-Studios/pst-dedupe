# ingest-purview

Purview / package / ZIP ingest for Dedupe Desk:

- **Detect** package kinds (`single_pst`, `single_zip`, `purview_package`, `raw_dump`, `unsupported`)
- **Safely expand** ZIPs into a matter CAS + inventory
- **Resumable leaf checkpoints** (mega-zip safe)
- **Legacy ZIP name encoding** fallbacks (UTF-8 → CP437 → Windows-1252)
- Audit events + `item_errors` for honest partial success

Depends on [`matter-core`](../matter-core). Does **not** open PSTs with `pst-reader` (register/discover only).

---

## ⚠ Blocking-thread caller contract

`ingest_path` and `resume_ingest` are **CPU- and IO-bound** and block for the full expand.

**Callers must run them on a dedicated blocking worker:**

- `std::thread::spawn`
- rayon thread pool
- `tokio::task::spawn_blocking` (when 0019+ introduces async orchestration)

**Do not** call them on:

- the egui / GUI thread
- a Tokio multi-thread worker (async executor thread)

This crate does not enforce the contract; track **0019** owns the process/job pool.

---

## Public API

```rust
use ingest_purview::{detect, ingest_path, resume_ingest, ExpandLimits, PackageKind};
use matter_core::Matter;

let kind = detect(path)?;
let summary = ingest_path(&matter, path, &ExpandLimits::default(), None)?;
// after cancel / crash:
let summary = resume_ingest(&matter, &source_id, &job_id, &limits, Some(&|| cancel_flag.load()))?;
```

| Type / fn | Role |
|---|---|
| `PackageKind` | `single_pst` / `single_zip` / `purview_package` / `raw_dump` / `unsupported` |
| `ExpandLimits` | Bomb limits + checkpoint cadence |
| `detect` | Best-effort classification (prefer `raw_dump` over false Purview) |
| `ingest_path` | Register source + ingest job + expand |
| `resume_ingest` | Continue from stage `expand` checkpoint + inventory skip |

---

## Default limits

| Limit | Default |
|---|---|
| `max_uncompressed_bytes` | 50 GiB |
| `max_compression_ratio` | 100.0 |
| `max_entries` | 500_000 |
| `max_zip_depth` | 8 |
| `checkpoint_every_n_entries` | 50 |
| `checkpoint_every_bytes` | 64 MiB |
| `max_entry_buffer_bytes` | 256 MiB |

Tests use `ExpandLimits::for_tests()` (tiny N, 1-entry checkpoints).

---

## Resume grain (mega-zip)

Checkpoint stage name: **`expand`**.

After each successful **leaf** CAS put + inventory insert (or every N entries / X bytes), the job checkpoint records:

- `last_successfully_extracted_logical_path`
- `completed_count`, `bytes_extracted`
- optional `archive_stack` / `completed_top_level`

**Resume is inventory-authoritative:** if `(source_id, path)` already has `native_sha256` and status `expanded`/`discovered`, a **non-container leaf** is **skipped** (no re-`put_bytes`). Nested **`.zip` containers** that are already inventored still **re-walk children** (bytes loaded from CAS, or re-read from the parent entry on CAS miss) so mid-nested cancel does not drop remaining leaves.

Cancel sets source `paused` + job `Paused` so resume can continue.

---

## ZIP name encoding

1. UTF-8 flag / valid UTF-8 bytes → UTF-8  
2. Else CP437 (ZIP historical default)  
3. Else Windows-1252 / Latin-1-style single-byte map  

**General-purpose bit 11 approximation:** the `zip` crate does not always expose
the raw GP bit 11 (language encoding flag). We treat a name as UTF-8 when
`name_raw` is valid UTF-8 **and** matches `ZipFile::name()`. Non-UTF-8 raw
names still fall through CP437 → Windows-1252. This is an intentional
approximation, not a full bit-11 parse.

Result is always a **UTF-8** logical path in SQLite. Paths are never rejected solely for encoding.

After decode: reject `..`, absolute paths, reserved device names, control chars.

Nested logical paths use `archive!/inner` style, e.g. `files.zip!/inner.zip!/note.txt`.

---

## Out of scope

| Deferred | Work |
|---|---|
| 0017 | Full Normalized Item / logical_hash |
| 0018 | PST open + message extract |
| 0019 | Job runner / `spawn_blocking` pool |
| — | 7z expand (recorded as `unsupported_7z`) |
| — | Mutating source export trees |

---

## Tests

```powershell
cargo test -p ingest-purview
```
