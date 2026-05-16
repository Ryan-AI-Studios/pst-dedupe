# Track 003 Spec: Dedup Tier Semantics

## Expected Behavior

- Tier 1 key is normalized Message-ID.
- Messages with the same normalized Message-ID are duplicates.
- Messages without Message-ID use content hash only when Tier 2 is enabled.
- Messages with different Message-ID values are not merged by Tier 2 unless the product explicitly chooses cross-tier matching later.

## Edge Cases

- Message-ID casing, whitespace, and angle brackets.
- Empty or malformed Message-ID values.
- Unicode subject and sender values.
- Missing submit time, sender, body, or attachment metadata.
- Attachment metadata in different source order must hash consistently when attachment hashing is enabled.
- Dependency update to hashing or time crates must not silently change duplicate decisions.

## Verification

- `cargo test -p dedup-engine`
- New tests around normalization, fallback, disabled Tier 2, and duplicate reporting.
