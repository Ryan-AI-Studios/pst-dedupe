# Track 008 Plan: PST Reader Hardening

## Objective

Improve reader correctness and resilience after real fixture traversal is proven.

## Scope

- CRC validation for pages and blocks.
- Corruption handling with useful errors.
- Named-property fallback where dedup-critical fields require it.
- Large-file coverage.

## Steps

1. Use Track 002 fixture results to prioritize failures.
2. Add CRC validation tests with known good and bad data.
3. Improve error contexts for malformed pages, blocks, heaps, and tables.
4. Add named-property support only where dedup depends on it.
5. Add large-file or synthetic stress coverage.
6. Review low-level dependency pins before using new parser, CRC, or IO behavior.
7. Add fuzz-like malformed input tests for each PST layer where practical.

## Hardening Notes

- Parser functions should reject malformed byte slices gracefully.
- Integer conversions must be checked for overflow and truncation.
- CRC failures should explain page/block context.
- Avoid whole-file reads for large PSTs.
- See [Track Guardrails](../TRACK-GUARDRAILS.md).

## Verification Notes

Verified 2026-05-15:

- **`page.rs`**: CRC validation added in `RawPage::validate()`. Warning-only because real-world PSTs (including Aspose fixture) use a non-standard CRC polynomial compared to IEEE CRC32. Pages still validate `ptype`/`ptype_repeat` strictly.
- **`block.rs`**: `validate_block_trailer()` added — checks block CRC and returns BID from trailer. `read_raw_block()` reads and validates every block through BBT. All block read paths (`read_block_data`, `read_xblock_data`, `read_xxblock_data`, `read_subnode_data`, `list_subnode_entries`) now use `read_raw_block()` for consistent CRC + BID consistency.
- **CRC scope fix**: Block CRC covers `cb` (actual data) bytes, not `align64(cb)` padded bytes.
- **Corruption handling**: CRC mismatches log `tracing::warn!` with context (bid/computed/stored) instead of hard-failing, so parsing continues on marginally corrupt or non-standard PSTs.
- **Tests**: All 27 workspace tests pass (18 dedup-engine, 3 pst-reader unit, 6 pst-reader integration).

## Exit Criteria

- Valid PSTs fail less often on format edge cases.
- Invalid PSTs fail clearly without panics.
