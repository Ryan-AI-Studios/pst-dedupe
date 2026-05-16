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

## Verification Notes

Verified on 2026-05-15:

- Added `tier2_enabled: bool` to `DedupIndex` with constructors `with_tier2()` and `with_capacity_and_tier2()`.
- `check_and_insert` skips Tier 2 lookup/insertion when disabled, avoiding the old worker hack of passing dummy `[0; 32]` hashes.
- Updated `worker.rs` to use `with_capacity_and_tier2(100_000, config.enable_tier2)` and removed the conditional `check_and_insert` branch.

**Tests added** (18 total in dedup-engine):
- `test_tier2_disabled_skips_content_hash` — same content hash = unique when Tier 2 off
- `test_tier1_priority_over_tier2` — same MID but different hash still matches Tier 1
- `test_empty_message_id_treated_as_missing` — empty string falls through to Tier 2
- `test_tier2_disabled_empty_mid_is_unique` — empty MID + Tier 2 off = unique
- `test_cross_tier_no_false_positive` — content hash matches across MID/no-MID (acceptable conservative behavior)
- `test_content_hash_missing_fields_stable` — all-None produces deterministic hash
- `test_content_hash_attachment_ordering` — reversed attachment order yields same hash
- `test_content_hash_unicode_subject` — "Re: Réunion" normalizes to "réunion"
- `test_content_hash_none_vs_empty_subject` — None and empty subject hash identically

## Exit Criteria

- Dedup tier behavior is documented by focused tests.
- GUI and worker config can rely on the same engine semantics.
