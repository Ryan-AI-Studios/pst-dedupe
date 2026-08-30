# Track Completion Audit — 0114-PdfRasterRedact

## Verdict: FAIL

## Scope Reviewed

Read all of `spec.md` and `plan.md`. Reviewed tracked, unstaged, and untracked product files in the execution repository. Conductor registry completion and DoD-6 were excluded as instructed.

## Requirement and DoD Matrix

| Requirement | Result |
|---|---|
| DoD-1 raster, navigation, errors, rotation | Fail: LRU loses truncation state |
| DoD-2 draft geometry separation | Pass by code/tests |
| DoD-3 true burn and burned-native resolution | Pass by code/tests |
| DoD-4 text-redaction honesty | Pass by code/tests |
| DoD-5 schema, permissions, CSP, production safety | Engineering wiring present; full gates not independently runnable |
| DoD-6 recorded completion | Out of scope |

Prior findings:

- Long-side flag: cold-render assignment is fixed, but cache-hit handling remains defective.
- Rotate-90 host path: implementation and reported targeted test are present.
- Raster warning severity: warning cards now render as `card warn` and show severity.
- Recorded completion: excluded per instruction.

## Findings

[P2] Raster LRU loses the long-side cap state on cache hits

Confidence: High

Requirement: DoD-1; `spec.md` §3.3 requires an honest truncated banner whenever the 4096-pixel cap applies.

Location: `crates\pdf-raster\src\lib.rs:78-86, 368-403`

Problem: `RasterPage` correctly carries `truncated`, and the cold PDF render sets it. However, `CacheEntry` does not store the field. Cache hits reconstruct the response with `truncated: false`.

Evidence:

- Cap assignment: `crates\pdf-raster\src\lib.rs:312-316`
- Cache entry omits the field: `crates\pdf-raster\src\lib.rs:78-86`
- Cache-hit response hard-codes `false`: `crates\pdf-raster\src\lib.rs:368-381`
- Cache insertion omits the field: `crates\pdf-raster\src\lib.rs:392-403`
- UI renders the banner only when true: `crates\dedupe-chrome\ui\src\pages\review_window.rs:992-994`

Failure scenario: A capped PDF page is rendered once, then revisited with the same native digest, page, and DPI. The LRU returns the PNG but reports `truncated=false`, so the reviewer sees a capped preview without the required warning.

Correction: Persist `truncated` in `CacheEntry`, clone it, store it, and return it on cache hits. Add a regression test that renders the same capped page twice using a nonempty cache key and asserts both responses are truncated.

Verification: The existing `pdf_long_side_cap_sets_truncated` test only uses `native_sha256=None`, so it bypasses the cache and does not cover this failure.

Deferrable: No

## Completeness Sweep

The following were independently confirmed:

- Schema version 40 and appended item columns.
- Separate geometric-redaction table and active-count bookkeeping.
- Native-change invalidation and stale geometry handling.
- Fingerprint includes native, geometry, text-redaction state, and engine pin.
- PDF burn uses incremental write, parse, then `rewrite_pdf`.
- Original native CAS remains untouched.
- JPEG remains JPEG; PNG remains PNG.
- Host commands use `join_worker`.
- Generation and item guards are present for raster/list/update flows.
- Encrypted matter and encrypted PDFs have explicit handling.
- Old Image-tab stub text is gone.
- No PDF bytes are sent to the UI; raster uses PNG data URLs.
- No `fs:default` capability or zpdf dependency in the UI.
- No new production `unwrap()`/`expect()` occurrences were found in the reviewed implementation.

## Wiring and Regression Review

The rotation transform, host upsert path, burn path, warning rendering, QC rules, resolver, permissions, CSP, and production copy are wired consistently. The reported rotation test specifically covers visible-token burn with neighbor preservation.

The remaining cache defect is independent of the reported cold-render cap test and affects the user-facing safety indication on subsequent page visits.

PDFs over 500 pages were not treated as a failure, per instruction.

## Verification Evidence

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- `cargo test -p pdf-raster`: attempted but blocked before execution by `C:\dev\Dedupe\target\debug\.cargo-lock` access denied.
- Rotation and cap targeted tests: orchestrator-reported passed; not claimed as independently run.
- Ledgerful status was unavailable because its database could not be opened; no files were modified.

## Deferred Candidates

None. The remaining issue is P2 and cannot be deferred.

## Completion Decision

FAIL. Fix the LRU truncation propagation, add the repeated-cache regression test, and rerun the permitted targeted gates.