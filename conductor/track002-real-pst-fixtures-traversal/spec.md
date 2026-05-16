# Track 002 Spec: Real PST Fixtures And Traversal

## Problem

The project cannot claim functional PST deduplication until the parser is exercised against real PST files. Unit tests around byte structures are necessary, but not enough to validate the file format layers working together.

## Expected Behavior

- The reader accepts Unicode PST files and rejects unsupported PST formats with clear errors.
- Folder traversal starts from the PST root folder and walks child folders deterministically.
- Message traversal discovers messages from contents tables.
- Dedup-relevant properties can be extracted for each message when present.
- Test fixtures are never committed if they contain real or private email data.

## Edge Cases

- Empty PST with folder structure but no messages.
- PST with nested folders and empty folders.
- Missing or malformed Message-ID, sender, date, body, and recipients.
- Unicode subjects, senders, folder names, and file paths.
- Unsupported ANSI PST and corrupted Unicode PST.
- PST larger than available memory for whole-file loading.

## Non-Goals

- Do not require public fixture files to exist before the test harness lands.
- Do not implement every named property or attachment edge case in this track.
- Do not optimize large-file performance yet; this track is about correctness proof.

## Verification

- `cargo test -p pst-reader`
- Fixture-backed ignored or opt-in integration tests.
- Manual smoke command or test instructions documented in README or conductor notes.
