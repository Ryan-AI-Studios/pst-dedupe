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

## Exit Criteria

- Valid PSTs fail less often on format edge cases.
- Invalid PSTs fail clearly without panics.
