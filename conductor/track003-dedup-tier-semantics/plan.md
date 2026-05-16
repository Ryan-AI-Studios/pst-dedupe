# Track 003 Plan: Dedup Tier Semantics

## Objective

Make Tier 1 and Tier 2 deduplication behavior explicit, correct, and covered by tests.

## Scope

- Confirm how configuration enables or disables Tier 2.
- Ensure Message-ID duplicates use Tier 1.
- Ensure missing Message-ID records fall back to Tier 2 only when enabled.
- Ensure Tier 2 does not override valid Tier 1 identity.

## Steps

1. Read `dedup-engine` hasher and index behavior.
2. Add tests for Message-ID normalization.
3. Add tests for content hash fallback with missing Message-ID.
4. Add tests for Tier 2 disabled behavior.
5. Add edge-case tests for malformed IDs, Unicode subjects, missing body, and attachment ordering.
6. Review `sha2` and date/time pins if hash inputs or time handling change.
7. Adjust implementation to match documented semantics.

## Hardening Notes

- Hash behavior must be deterministic across platforms and Rust versions.
- Do not change existing dedup decisions without adding migration notes to reports/docs.
- Missing fields must produce stable fallback inputs rather than panics or nondeterministic hashes.
- See [Track Guardrails](../TRACK-GUARDRAILS.md).

## Exit Criteria

- Dedup tier behavior is documented by focused tests.
- GUI and worker config can rely on the same engine semantics.
