# Track Completion Audit — 0076-ContentHashTierHardening

## Verdict: FAIL

## Prior findings

All seven prior findings are fixed in their original scope:

- F-0076-01: `dups` now builds shared grouping context.
- F-0076-02: GUI worker uses `GroupingContext`, `IndexItem`, and `check_and_insert_item`.
- F-0076-03: streaming `scan`/`dups` reject backfill; keep-set/unique paths perform it.
- F-0076-04: divergence is recorded before optional splitting.
- F-0076-05: clean empty bodies preserve `Some("")` presence.
- F-0076-06: attribution runs after backfill and excludes ineligible items.
- F-0076-07: `unique-eml` prints grouping statistics.
- The dual-identical-input test is present and updated.

## Findings

[P1] `tier1-backfill` violates `per-source` scope

Confidence: High  
Requirement: DoD-8, DoD-9, spec §3.8  
Location: [keepset.rs:1136](C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:1136)

`apply_tier1_backfill_pass` groups only by v1 content hash and unions compatible groups without considering source scope. Consequently, `--tier1-backfill --dedupe-scope per-source` can merge groups from different PSTs, defeating custodial dedupe.

Correction: Partition backfill candidates by `path_key` when scope is `PerSource`, and add a combined-context regression test.

Deferrable: No.

[P2] Backfill leaves bind provenance stale

Confidence: High  
Requirement: DoD-5, honest binding provenance  
Locations: [keepset.rs:980](C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:980), [keepset.rs:2086](C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:2086)

The post-pass merges groups but does not update `bound_by`. A former group seed can become a final duplicate while retaining `BoundBy::Seed`, causing blank/incorrect duplicate tier reporting and undercounted Tier-2 statistics.

Deferrable: No.

## Deferred/residual items

The explicit `body-recip-attach` rejection is honest and documented. Fixture-wide baselines, full GUI strong-hash controls, and performance timings remain evidence gaps; these are limited residuals, but they do not excuse the P1 scope defect.

## Verification evidence

Observed:

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- Relevant prebuilt tests for backfill, equivalence, refinement, divergence, empty-body handling, attach rejection, reader behavior, and GUI checkbox: passed.
- Full Cargo tests/clippy were blocked by `Access is denied` on `target\debug\.cargo-build-lock`; file-writing tests were also blocked by read-only temp directories.
- Ledgerful status/impact unavailable due database/report write restrictions.
- `review.md` remains a scaffold and conductor status remains **In Progress**.

No files or Git state were modified.