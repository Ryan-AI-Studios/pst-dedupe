# Track 003 TDD

## Red

- Write failing tests for Tier 1 exact behavior and Tier 2 fallback behavior.
- Write failing tests for missing fields, malformed Message-ID, Unicode text, and attachment ordering.

## Green

- Implement the smallest changes in `dedup-engine` to pass the tier tests.

## Refactor

- Move shared normalization/hash helpers behind clear APIs if tests expose duplication.
- Add golden hash expectations for representative messages before changing hash-related dependencies.
