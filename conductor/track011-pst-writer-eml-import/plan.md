# Track 011 Plan: Minimal PST Writer for EML-to-PST Fixture Generation

## Objective

Build a minimal Rust PST writer that imports EML files into a valid Unicode PST fixture. This enables integration testing with real email content.

## Scope

- New crate: `pst-writer`
- Create Unicode PST from scratch (unencrypted, no compression)
- Single folder structure (root + PROMOTIONS)
- Import 41 EML files
- Store: Message-ID, Subject, Sender, Date, Body
- No attachments, no named properties, no subnodes for messages
- Pre-calculated file layout (no dynamic allocation)

## Implementation Phases

1. **Foundation:** layout calculator, block/page writers
2. **LTP Builders:** HN, BTH, PC, TC
3. **EML Parser:** MIME header extraction, body extraction, date conversion
4. **Messaging Layer:** Store PC, folder PCs, hierarchy TC, contents TC, message PCs
5. **Integration:** Wire everything, write `fixtures/promotions_spam.pst`
6. **Verification:** Open with `PstFile`, read all 41 messages, run cargo gate

## Exit Criteria

- [x] `fixtures/promotions_spam.pst` created from 41 EMLs (195,472 bytes)
- [x] `pst-reader` opens it and reads all 41 messages with correct properties
- [x] `cargo test --workspace` passes
- [x] `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass
- [x] Body truncation to 2000 chars prevents `u16` overflow in `BBTENTRY.cb`
- [x] `MAX_BLOCK_DATA` assertion guards against oversized blocks
- [x] Debug/temporary test files removed

## Verification Log

- 2026-05-16: All workspace tests pass (34 tests across dedup-engine, pst-reader, pst-writer)
- 2026-05-16: Integration test `test_create_pst_from_eml` confirms 2 folders, 41 messages
- 2026-05-16: Clippy clean (`-D warnings`)
- 2026-05-16: Fixture copied to `fixtures/promotions_spam.pst`

## Key Bugs Fixed

1. **`DataTruncated { needed: 32, available: 776 }`**: Root cause was EML body (169KB) causing message PC heap to exceed `MAX_BLOCK_DATA`. `BBTENTRY.cb` is `u16`, so `328456 as u16 = 776`, corrupting block size. Fix: truncate body to 2000 chars and add `assert!` in `add_node`.
2. **Table NID computation**: Changed from `nid | type` to `(nid & !0x1F) | type` to preserve base NID index.
3. **HN block_size for single-block nodes**: Use `data.len()` instead of `MAX_BLOCK_DATA` when node fits in one block.
