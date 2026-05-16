# Track 008 Spec: PST Reader Hardening

## Expected Behavior

- Parser validates PST integrity where the spec provides checks.
- Corrupted files produce structured errors.
- Large Unicode PSTs can be scanned without integer truncation or runaway memory use.

## Edge Cases

- Bad page trailer repeat type, CRC, signature, or BID.
- XBLOCK/XXBLOCK cycles, missing child blocks, and inconsistent sizes.
- Heap allocation maps with invalid offsets.
- Table rows with missing or malformed columns.
- FILETIME and UTF-16 decoding edge cases.
- Dependency update changes byte-order, CRC, or error trait behavior.

## Verification

- `cargo test -p pst-reader`
- Fixture-backed corruption tests.
- Large-file smoke test when fixtures exist.
