# Track Completion Audit — 0114-PdfRasterRedact

## Verdict: FAIL

## Scope Reviewed

Reviewed all of `spec.md` and `plan.md`, tracked changes, and untracked product files on the working tree. Conductor registry completion was excluded.

## Requirement and DoD Matrix

| Area | Result |
|---|---|
| Schema v40, append-only columns, CRUD, audit, fingerprints | Implemented |
| Raster preview, caps, encryption handling, generation guards | Implemented |
| PDF content-stream burn and rewrite | Implemented |
| JPEG/PNG paint burn and codec preservation | Implemented |
| Produce fail-closed behavior | Not complete; partial text coverage can ship |
| QC rules and selected-set wiring | Implemented, with counter/UI issue below |
| Required permissions and dependency boundaries | Implemented |
| DoD-1 | Partial: host coordinate authority issue |
| DoD-2 | Met |
| DoD-3 | Fails: second text redaction can remain in burned native |
| DoD-4 | Zero-geometry case met; partial mapping case fails |
| DoD-5 | Code present; full post-r2 test gate not independently runnable |
| DoD-6 | Excluded per instruction |

## Findings

[P1] Existing geometry allows newly added text redactions to be certified as burned

Confidence: High

Requirement: DoD-3 variant; spec §3.5 and §3.6 require text changes to remain stale/unmapped until their content is covered.

Location: `crates\dedupe-chrome\src\raster.rs:366`, `crates\dedupe-chrome\src\raster.rs:383`

Problem: Mapping is tracked only as an aggregate geometry count. `review_geom_from_hits` and `burn_one_item` treat any existing geometry as sufficient. There is no relationship between an active 0032 text redaction and the geometry that supposedly covers it.

Evidence: `burn_one_item` blocks only when `redaction_count > 0 && geom_redaction_count == 0`, then burns all existing geometry and persists the current fingerprint.

Failure scenario:

1. Map and burn text token A.
2. Add text redaction token B without adding geometry for B.
3. Regenerate the redacted text artifact.
4. Burn again.

The second burn processes only the old geometry for A, but `set_burned_native` records a fingerprint containing B. Production then accepts the burned PDF even though token B remains in its native content.

The existing `post_burn_text_redaction_stale_refuses` test does not catch this because it intentionally omits redacted-text regeneration and therefore can fail earlier with `redacted_text_missing`.

Correction: Require proof that every active text redaction is covered before certifying the burned fingerprint. Track mapping coverage or re-run matching for every active exact quote and fail closed for unmatched text. Add a regression test asserting the second token remains blocked or is removed from the produced native.

Verification: Confirmed by static call-graph inspection. Cargo tests could not run because Cargo was denied access to `target\debug\.cargo-lock`.

Deferrable: No

[P2] PDF coordinate conversion trusts client-supplied raster dimensions

Confidence: High

Requirement: Spec §3.3 and DoD-1 require a host-owned pixel-to-PDF coordinate map.

Location: `crates\dedupe-chrome\src\raster.rs:229`

Problem: The host re-rasterizes the page and obtains authoritative `page.width` and `page.height`, but PDF conversion uses `args.raster_width` and `args.raster_height` supplied by the UI. A stale or malformed IPC request can therefore persist incorrect PDF user-space coordinates.

Failure scenario: A drag generated from an old preview, or an IPC caller supplying mismatched dimensions, maps to the wrong PDF rectangle. Burn may redact a neighboring region or miss the visible secret.

Correction: Derive PDF mapping dimensions from the host’s `RasterPage` result, or reject client dimensions that do not exactly match the current raster dimensions. Add a rotated/CropBox host-command test with deliberately mismatched dimensions.

Verification: Static inspection. The normal UI path currently sends matching dimensions, but the host does not enforce that invariant.

Deferrable: No

[P2] Burn counters remain for the default set after selecting the entire corpus

Confidence: High

Requirement: Spec §3.6 requires Burn-step counts for the current produce set.

Location: `crates\dedupe-chrome\src\produce.rs:549`, `crates\dedupe-chrome\ui\src\pages\produce.rs:370`

Problem: `produce_page_blocking` computes burn counts only for the default filtered set. Selecting “Entire review corpus” changes QC and Burn selection, but the UI does not refresh these counters from the QC ordered IDs. The page continues displaying default-set `need_burn` and `burned_fresh` values.

Evidence: The QC response is stored at `produce.rs:94`, but no page refresh or selected-set counter calculation follows. The Burn action itself uses the QC IDs correctly.

Failure scenario: The user selects the entire corpus, runs QC, and burns it. The displayed counts can underreport required burns or fresh burns for items outside the default set.

Correction: Calculate counters from the current QC `ordered_ids`, or expose a selected-set counter endpoint and refresh it after QC selection changes.

Verification: Static inspection.

Deferrable: No

## Completeness Sweep

The implemented path contains no remaining raster stub, `iw.document()` burn shortcut, Chrome `process-runner` dependency, GPU renderer, or zpdf dependency in `ui/`.

The following prior fixes remain present:

- Burn persistence checks expected fingerprint, CAS existence, and digest inequality.
- Image preview exposes native dimensions and maps capped previews to native pixels.
- Burn requires nonempty QC-selected IDs in the UI.
- PDF-native detection reads a CAS prefix.
- Image truncation displays the 4096-pixel banner.
- Raster/document/page generation guards are present.
- Rotate coordinate transforms and zero-hit handling are present.
- Schema v40 fields are appended at the end of `ITEM_COLUMNS`.

## Wiring and Regression Review

The engine is reachable through Chrome commands, permissions, produce Burn, `resolve_native`, and QC. The true PDF path uses incremental redaction followed by parse and rewrite, preserving the original CAS blob.

The critical regression is between text-redaction state, geometry coverage, and burn certification: the fingerprint detects the text change, but the burn implementation can certify a native that only reflects older geometry.

## Verification Evidence

Observed:

- `cargo fmt --all --check` — passed.
- `git diff --check` — passed.
- `cargo metadata --no-deps` — confirmed `pdf-raster` is in the workspace and `dedupe-chrome` depends on it.
- Targeted `cargo test -p pdf-raster` — blocked before compilation: `Access is denied` opening `C:\dev\Dedupe\target\debug\.cargo-lock`.

The r2 targeted test results supplied in the request were treated as orchestrator-reported, not independently observed. The post-r2 workspace test was not rerun.

Ledgerful and ai-brains checks were unavailable in this read-only environment due Ledgerful database access and missing vault-key configuration.

## Deferred Candidates

None. The findings are engineering defects, not qualifying deferred P3 items. The planned PDF page-cap residual was not treated as a failure.

## Completion Decision

FAIL. The track is not complete until text-redaction coverage is bound to burn certification and the resulting burned native cannot retain a newly added text redaction.