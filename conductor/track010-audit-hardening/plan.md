# Track 010 Plan: Security & Robustness Hardening

## Objective

Address HIGH and MEDIUM findings from the comprehensive code audit (`docs/audit.md`) to harden the GUI worker, prevent panics on Unicode strings, and clean up duplicated helpers.

## Scope

- **SEC-01 (HIGH):** Worker thread panic poisons `Arc<Mutex<ScanProgress>>` and crashes GUI.
- **SEC-02 (MEDIUM):** 9 instances of `progress.lock().unwrap()` in worker/GUI — replace with poison recovery.
- **SEC-03 (MEDIUM):** Byte-index string truncation (`&s[..80]`) panics on multi-byte UTF-8 in EML exporter and results view.
- **SEC-04 (MEDIUM):** Body preview truncation in `message.rs` uses byte slice on `String`.
- **SEC-05 (MEDIUM):** EML filenames preserve spaces and lack path-traversal defense.
- **QA-04 (LOW):** FILETIME conversion logic duplicated across `report.rs` and `worker.rs`.
- **QA-05 (LOW):** `format_size`/`format_bytes` duplicated between `report.rs` and `results.rs`.

## Steps

1. Extract shared helpers into `dedup-engine`:
   - `filetime_to_unix(ft: i64) -> i64` — single FILETIME→Unix conversion.
   - `format_bytes(bytes: u64) -> String` — human-readable size.
   - `truncate_utf8(s: &str, max_chars: usize) -> String` — safe Unicode truncation.
2. Fix `dedup-engine/src/exporter.rs`:
   - Replace byte truncation with `truncate_utf8`.
   - Replace spaces with underscores in filenames.
   - Strip path separators (`/` and `\`).
3. Fix `pst-reader/src/messaging/message.rs`:
   - Replace `b[..4096].to_string()` with `truncate_utf8(&b, 4096)`.
4. Fix all `progress.lock().unwrap()` calls:
   - `app.rs` (start_scan)
   - `worker.rs` (9 instances)
   - `progress.rs` (2 instances)
   - Replace with `lock().unwrap_or_else(|e| e.into_inner())`.
5. Fix `results.rs`:
   - Replace local `truncate` and `format_size` with imports from `dedup-engine`.
6. Update `dedup-engine/src/lib.rs` to re-export helpers.
7. Run full verification gate.

## Verification Notes

Verified 2026-05-16:

- **Mutex poisoning (SEC-01, SEC-02):** Replaced all 11 `progress.lock().unwrap()` calls across `app.rs`, `worker.rs`, and `progress.rs` with `lock().unwrap_or_else(|e| e.into_inner())`. This recovers the guard data even if poisoned, preventing cascading GUI crashes.
- **Unicode truncation (SEC-03, SEC-04):**
  - Added `dedup-engine::util::truncate_utf8()` — truncates by character count with "..." suffix, safe for multi-byte UTF-8.
  - Updated `exporter.rs::make_eml_filename()` to use `truncate_utf8(..., 80)`.
  - Updated `message.rs::read_message_properties()` to use `chars().take(4096).collect()` for body preview.
  - Removed local `truncate()` and `format_size()` from `results.rs`; now imports `truncate_utf8` and `format_bytes` from `dedup-engine`.
- **EML filename hardening (SEC-05):** `make_eml_filename()` now filters out `/` and `\` path separators and replaces spaces with underscores.
- **Helper deduplication (QA-04, QA-05):**
  - Extracted `filetime_to_unix()` to `dedup-engine::util` and used it in `worker.rs` and `report.rs`.
  - Extracted `format_bytes()` to `dedup-engine::util` and used it in `report.rs` and `results.rs`.
- **Tests:** Added 6 unit tests in `util.rs` covering FILETIME conversion, format_bytes, and Unicode truncation (including multi-byte Japanese and boundary cases).
- **All gates pass:** `cargo fmt`, `cargo clippy -Dwarnings`, `cargo test --workspace`, `changeguard verify` all green. 33 tests pass (24 dedup-engine, 3 pst-reader unit, 6 integration).

## Exit Criteria

- All `.unwrap()` on mutex locks recover from poisoning.
- No byte-index truncation on `String` values.
- EML filenames have no spaces and no path separators.
- FILETIME and format_size helpers are deduplicated.
- Full gate passes: fmt, clippy, test, changeguard verify.
