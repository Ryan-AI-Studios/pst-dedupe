# PST-Dedup Comprehensive Code Audit

**Date:** 2026-05-16
**Auditor:** Hermes Agent
**Scope:** Full workspace as of audit date (`pst-reader`, `dedup-engine`, `pst-dedup-gui`)
**Commit:** HEAD (uncommitted changes in `track/022-unified-stealth` branch from external repo — audit covers `c:/dev/dedupe` in isolation)

> **Historical note (2026-07):** This audit is a point-in-time snapshot. Since then the workspace gained `pst-dedup-cli` and `pst-writer`, migrated provenance tooling from ChangeGuard to **Ledgerful** (`.ledgerful/`), and fixed reader bugs (Unicode `bCryptMethod` alignment, HNPAGEMAP `cFree`, TC RowIndex NIDs) proven on a real Permute multi-mailbox PST. Re-audit before treating findings as current severity.

---

## 1. Executive Summary

PST-Dedup is a well-architected Rust workspace implementing a pure-R PST (Personal Storage Table) parser and email deduplication engine. The code demonstrates strong domain knowledge of the MS-PST specification, disciplined layering (NDB → LTP → Messaging), and a clean separation between format parsing and business logic.

**Overall Grade: B+** — Production-ready with reservations. Core parsing logic is solid, but the GUI crate has threading hazards, error handling gaps, and several areas where malicious/corrupted PST inputs could trigger panics or resource exhaustion.

---

## 2. Architecture Assessment

### 2.1 Crate Separation

| Crate | Responsibility | Maturity |
|---|---|---|
| `pst-reader` | MS-PST format parser (NDB, LTP, Messaging) | High |
| `dedup-engine` | Hashing, indexing, CSV/EML export | High |
| `pst-dedup-gui` | egui frontend + background worker | Medium |

The separation is clean. `pst-reader` has zero knowledge of deduplication; `dedup-engine` has zero knowledge of PST internals. This is architecturally sound and allows independent reuse.

### 2.2 Layering (MS-PST Compliance)

The parser faithfully follows the spec layering:

- **NDB:** Page reading, B-tree traversal, block assembly (XBLOCK/XXBLOCK), subnode B-trees, encryption
- **LTP:** Heap-on-Node, BTree-on-Heap, Property Context, Table Context
- **Messaging:** Store, folder hierarchy, message properties, attachment metadata

The `ARCHITECTURE.md` document is excellent — it functions as both design doc and implementation guide, with byte-level layout tables and spec references.

### 2.3 Notable Design Decisions

- **Full in-memory NBT/BBT indexes:** Built once at file open via recursive B-tree traversal. This is fast and simple but means opening a large PST allocates two HashMaps proportional to node/block count. For a 50GB PST this is still only ~tens of MB.
- **BufReader with 64KB capacity:** Good default for sequential block reads.
- **Tiered dedup:** Message-ID (Tier 1) → SHA-256 content hash (Tier 2). This is the correct industry approach.

---

## 3. Security Assessment

### 3.1 Threat Model

The primary attack surface is **malicious or corrupted PST files** fed into the parser. Secondary surfaces: EML export path traversal, GUI worker thread crashes, and resource exhaustion.

### 3.2 Findings

#### 🔴 HIGH: Worker Thread Panic = GUI Crash

**Location:** `pst-dedup-gui/src/app.rs:117`

```rust
match handle.join() {
    Ok(result) => { ... }
    Err(_) => {
        self.error_msg = Some("Worker thread panicked".into());
        self.state = AppState::FileSelect;
    }
}
```

The worker thread runs parser code that uses `.unwrap()` in several places (see below). If the worker panics, `handle.join()` returns `Err`, but `ScanResult` is lost. More critically, the `Arc<Mutex<ScanProgress>>` could be poisoned if the panic occurred while the lock was held, causing subsequent `lock().unwrap()` calls in the main GUI thread to **also panic**.

**Recommendation:** Use `std::sync::Mutex` poisoning recovery (`into_inner()` on the guard) or switch to `parking_lot::Mutex` (no poisoning). Better yet, wrap the entire worker loop in `catch_unwind` and convert all panics to error messages.

---

#### 🟡 MEDIUM: Unwrap/Expect in Production Parser Code

**Location:** `pst-dedup-gui/src/worker.rs` (multiple)

The GUI worker contains 9 instances of `.unwrap()` on `progress.lock()`. While unlikely to fail in practice, any logic error that causes the mutex to be poisoned will abort the process.

**Locations in non-test code:**
- `app.rs:100` — `progress.lock().unwrap()`
- `worker.rs:93, 105, 123, 142, 158, 167, 246, 270` — `progress.lock().unwrap()`
- `progress.rs:7, 68` — `progress.lock().unwrap()`

**Recommendation:** Replace with `lock().unwrap_or_else(|e| e.into_inner())` to handle poisoned mutexes gracefully.

---

#### 🟡 MEDIUM: String Truncation on Byte Boundaries (Potential Panic)

**Location:** `dedup-engine/src/exporter.rs:64`

```rust
let truncated = if safe.len() > 80 { &safe[..80] } else { &safe };
```

This slices by byte index on a UTF-8 `String`. If `safe` contains a multi-byte UTF-8 character crossing the 80-byte boundary, this will **panic** at runtime.

**Also affected:** `results.rs:248-250`

```rust
fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max - 3])
    } else { ... }
}
```

**Recommendation:** Use `s.chars().take(n).collect()` for safe Unicode truncation.

---

#### 🟡 MEDIUM: Body Preview Truncation Assumes UTF-8

**Location:** `pst-reader/src/messaging/message.rs:48-50`

```rust
let body_preview = body_full.map(|b| {
    if b.len() > 4096 {
        b[..4096].to_string()  // may split multi-byte char
    } else {
        b
    }
});
```

Same byte-boundary issue on a `String`. If `b` is valid UTF-16LE decoded content that happens to have a multi-byte UTF-8 sequence at the 4096-byte mark, `.to_string()` on the slice will panic.

**Recommendation:** Use `b.chars().take(4096).collect()`.

---

#### 🟡 MEDIUM: EML Filename Sanitization Allows Spaces

**Location:** `dedup-engine/src/exporter.rs:52-62`

```rust
let safe: String = subject
    .chars()
    .map(|c| {
        if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c }
        else { '_' }
    })
    .collect();
```

Spaces are preserved in filenames. On Windows this is generally fine, but on some filesystems or when passed to shell commands, spaces cause parsing issues. More importantly, the function does not prevent path traversal if a subject somehow contains `../` (the `..` would be replaced with `__`, but this is a defense-in-depth gap).

**Recommendation:** Replace spaces with underscores and explicitly reject/escape path separators.

---

#### 🟢 LOW: CRC Warnings Instead of Hard Failures

**Location:** `pst-reader/src/ndb/page.rs:107-115`, `block.rs:50-57`

CRC mismatches are logged as `tracing::warn!` but do not abort parsing. This is an intentional design choice documented in the code ("real-world PSTs often deviate"), but it means a corrupted/malicious file that fails CRC checks will still be processed.

**Recommendation:** Acceptable for robustness, but consider adding a strict-mode flag for forensic use cases.

---

#### 🟢 LOW: No Input Size Limits

There are no explicit caps on PST file size, message count, or attachment count. A malicious file claiming enormous sizes in the header could cause memory exhaustion during B-tree traversal or block allocation.

However, the code does check bounds before indexing (e.g., `offset + 32 > data.len()`), so out-of-bounds reads are generally handled gracefully.

**Recommendation:** Add a configurable max-file-size check and `Vec` pre-allocation limits.

---

### 3.3 Safe Rust Verification

- **No `unsafe` blocks found** (verified via `grep -r unsafe crates/`)
- No `std::mem::transmute`, raw pointer derefs, or FFI calls in the parser
- All file I/O goes through `std::fs::File` and `BufReader`

The project is **100% safe Rust**, which materially reduces the attack surface.

---

## 4. Correctness & Specification Compliance

### 4.1 Header Parsing

- Magic validation (`!BDN` / `0x4E444221`) ✓
- Client magic (`SM` / `0x4D53`) ✓
- ANSI rejection (wVer < 23) ✓
- ROOT structure parsing (72 bytes Unicode) ✓
- Encryption method detection ✓

### 4.2 NDB Layer

- Page reading (512 bytes, 16-byte trailer) ✓
- B-tree traversal (recursive, NBT + BBT) ✓
- Block trailer validation (CRC + BID consistency check) ✓
- XBLOCK / XXBLOCK assembly ✓
- Subnode BTree (SLBLOCK / SIBLOCK) traversal ✓
- Encryption: PERMUTE and CYCLIC ✓ (verified with roundtrip tests)

### 4.3 LTP Layer

- Heap-on-Node parsing ✓
- HID resolution with bounds checking ✓
- BTree-on-Heap traversal ✓
- Property Context type resolution (fixed + variable size) ✓
- Table Context row iteration ✓
- UTF-16LE decoding with odd-byte handling ✓

### 4.4 Messaging Layer

- Folder hierarchy traversal (recursive via hierarchy TC) ✓
- Contents table message NID extraction ✓
- Message property extraction (subject, sender, body, Message-ID, etc.) ✓
- Attachment metadata (name + size) ✓

### 4.5 Dedup Engine

- Tier 1 Message-ID normalization (lowercase, trim angle brackets) ✓
- Tier 2 SHA-256 content hash (subject + time + sender + body + attachments) ✓
- Subject normalization (recursive Re:/Fwd:/FW: stripping) ✓
- Attachment sorting for hash stability ✓
- Tier 1 priority over Tier 2 ✓
- Empty Message-ID falls through correctly ✓

---

## 5. Performance & Scalability

### 5.1 Memory Usage

The `DedupIndex` holds all `MessageRef` structs for the lifetime of the scan. At 1M messages with the ARCHITECTURE.md estimate of ~200 bytes per `MessageRef` plus HashMap overhead, this is ~400–500MB. This is acceptable for a desktop workstation but may strain older machines.

**The `all_rows` vector in `worker.rs` also retains every `ReportRow`** — this is necessary for the CSV report, but doubles the memory footprint. For 1M messages, expect ~800MB–1GB total.

### 5.2 Throughput

The worker updates progress stats every 100 messages. For a typical PST, the bottleneck is file I/O (random block reads via `BufReader`). No benchmarking data is included in the repo, so actual msgs/sec is unverified.

### 5.3 GUI Threading

The GUI uses a single background thread (`std::thread::spawn`) and polls completion via `handle.is_finished()` every 100ms. This is simple and adequate, but:
- Only one scan can run at a time
- No parallelism across PST files (sequential processing)
- No async I/O

**Recommendation:** For enterprise-scale deployments (1M+ messages), consider:
1. Parallel folder traversal within a single PST (if file locking allows)
2. Streaming report generation instead of buffering all rows
3. Optional memory-mapped I/O for large files

---

## 6. Testing Coverage

### 6.1 Test Inventory

| Crate | Unit Tests | Integration Tests | Doc Tests |
|---|---|---|---|
| `pst-reader` | 3 (crypto roundtrips) | 6 (fixture-based) | 1 |
| `dedup-engine` | 18 (hasher + index) | 0 | 0 |
| `pst-dedup-gui` | 0 | 0 | 0 |

### 6.2 Coverage Gaps

**Critical missing tests:**
- NDB block assembly (XBLOCK/XXBLOCK) — no unit tests for multi-block data
- LTP Heap/BTH/PC/TC — all logic is tested only via integration tests with real PSTs
- Error paths (truncated data, invalid headers, missing nodes) — largely untested
- GUI worker thread — completely untested
- EML export roundtrip — only filename generation is tested
- CSV report generation — no tests

**Fixture dependency:** Integration tests require real PST files in `fixtures/`. The test runner skips gracefully when none exist, but this means CI may run zero integration tests.

**Recommendation:** Add synthetic PST generation for deterministic integration tests, and add unit tests for every `Err` return path in the parser.

---

## 7. Dependency Audit

### 7.1 Direct Dependencies

| Crate | Version | License | Purpose | Assessment |
|---|---|---|---|---|
| `byteorder` | 1.5 | MIT/Unlicense | LE reads | Stable, tiny |
| `thiserror` | 2 | MIT/Apache-2.0 | Error derives | Standard |
| `sha2` | 0.11 | MIT/Apache-2.0 | Tier 2 hashing | Standard |
| `csv` | 1.4 | MIT/Unlicense | Report writing | Standard |
| `chrono` | 0.4 | MIT/Apache-2.0 | Date formatting | Standard |
| `crc32fast` | 1.5 | MIT/Apache-2.0 | CRC validation | Fast, safe |
| `tracing` | 0.1 | MIT | Logging | Standard |
| `tracing-subscriber` | 0.3 | MIT | Log output | Standard |
| `eframe` | 0.34 | MIT/Apache-2.0 | GUI framework | Large dep tree |
| `rfd` | 0.17 | MIT | File dialogs | Safe |

Third-party dependency licenses are permissive (MIT/Apache-2.0/Unlicense). No GPL, LGPL, or copyleft dependencies. **The Dedupe product itself is proprietary commercial** (root `LICENSE`); a paid license is required to use it. Dependency stack remains suitable to ship under that commercial license.

### 7.2 Indirect Dependency Notes

`eframe` pulls in a large ecosystem (`wgpu`, `winit`, `zbus`, `accesskit`, etc.). This is unavoidable for a native GUI, but it significantly increases compile times and binary size. The `pst-dedup-gui` crate does not use `wgpu` rendering explicitly; the default egui renderer is fine.

---

## 8. Code Quality & Maintainability

### 8.1 Strengths

- **Excellent documentation:** Every module has top-level doc comments explaining the MS-PST spec section it implements.
- **Consistent error handling:** Custom `PstError` enum with `thiserror`, propagated via `?` throughout.
- **Defensive bounds checking:** Nearly all array accesses are guarded (e.g., `if offset + 24 > data.len() { break; }`).
- **No dead code:** All functions are used; no warnings under `cargo clippy --workspace`.

### 8.2 Weaknesses

- **Missing inline documentation on complex algorithms:** The B-tree traversal and HN page map resolution are correct but under-commented for future maintainers.
- **Magic numbers:** Some constants (e.g., `0x122` for root folder) are used inline rather than named constants in all locations.
- **Duplicate `format_size` / `format_bytes` functions:** Defined separately in `report.rs` and `results.rs`.
- **FILETIME conversion duplicated:** The Unix epoch offset (`11_644_473_600`) and conversion formula appear in `report.rs`, `worker.rs`, and implicitly in `hasher.rs`.

### 8.3 Clippy Status

`cargo clippy --workspace --all-targets -- -D warnings` **passes cleanly** (after compilation). No lint violations.

---

## 9. Findings Register

| ID | Severity | Category | Description | Location |
|---|---|---|---|---|
| SEC-01 | 🔴 HIGH | Reliability | Worker thread panic can poison mutex and crash GUI | `gui/src/app.rs:117` |
| SEC-02 | 🟡 MEDIUM | Reliability | 9× `Mutex::lock().unwrap()` in worker; poison crash | `gui/src/worker.rs` |
| SEC-03 | 🟡 MEDIUM | Correctness | Byte-index string truncation may panic on Unicode | `exporter.rs:64`, `results.rs:248` |
| SEC-04 | 🟡 MEDIUM | Correctness | Body preview truncation may panic on multi-byte UTF-8 | `message.rs:48-50` |
| SEC-05 | 🟡 MEDIUM | Security | EML filename allows spaces; weak path traversal defense | `exporter.rs:52-62` |
| SEC-06 | 🟢 LOW | Security | CRC mismatches are warning-only (intentional but noted) | `page.rs:107-115` |
| SEC-07 | 🟢 LOW | Security | No max file size or message count limits | `lib.rs:76` |
| QA-01 | 🟡 MEDIUM | Testing | No unit tests for XBLOCK/XXBLOCK assembly | `block.rs` |
| QA-02 | 🟡 MEDIUM | Testing | No tests for CSV report or EML export | `report.rs`, `exporter.rs` |
| QA-03 | 🟡 MEDIUM | Testing | GUI completely untested | `gui/src/` |
| QA-04 | 🟢 LOW | Maintainability | FILETIME conversion logic duplicated in 3 files | `report.rs`, `worker.rs` |
| QA-05 | 🟢 LOW | Maintainability | `format_size` duplicated between crates | `report.rs`, `results.rs` |

---

## 10. Recommendations

### Immediate (Before Production Release)

1. **Fix mutex poisoning:** Wrap worker entry point in `std::panic::catch_unwind` and/or switch GUI mutexes to `parking_lot::Mutex`.
2. **Fix Unicode truncation:** Replace all `&s[..n]` on `String` with `s.chars().take(n).collect()`.
3. **Harden EML filenames:** Replace spaces with underscores; explicitly strip `..` and path separators.
4. **Add bounds to input files:** Reject PSTs > configurable max size (e.g., 100GB) to prevent resource exhaustion.

### Short-Term (Next Sprint)

5. **Add synthetic PST generator:** A small binary that writes minimal valid Unicode PSTs for deterministic integration tests.
6. **Unit test error paths:** Every `PstError` variant should have a test that triggers it.
7. **Extract FILETIME helper:** Create a single `filetime_to_datetime` utility in `pst-reader` and reuse everywhere.
8. **Extract `format_size` helper:** Move to a shared utility or use the `humansize` crate.

### Long-Term (Roadmap)

9. **Streaming report generation:** Write CSV rows incrementally instead of buffering all `ReportRow` structs in memory.
10. **Parallel PST processing:** Process multiple PST files concurrently (each in its own thread) since they are independent.
11. **Add `criterion` benchmarks:** Measure block read throughput, B-tree traversal speed, and end-to-end msgs/sec.
12. **Consider `rayon` for parallel folder traversal:** Within a single PST, folders could be processed in parallel if `PstFile` were split into read-only index + mutable reader handle.

---

## 11. Conclusion

PST-Dedup is a competent, spec-compliant implementation of a non-trivial binary format parser. The core `pst-reader` crate is the strongest component — it reads real-world PSTs correctly, handles encryption, and degrades gracefully on malformed inputs. The `dedup-engine` is well-tested and algorithmically sound.

The `pst-dedup-gui` crate is the weakest link. It lacks tests, has threading robustness issues, and contains the majority of the unwrap/expect calls. These are all fixable without architectural changes.

With the **Immediate** recommendations addressed, this codebase is suitable for internal deployment. With the **Short-Term** recommendations addressed, it is suitable for production release to end users.

---

*End of Audit*
