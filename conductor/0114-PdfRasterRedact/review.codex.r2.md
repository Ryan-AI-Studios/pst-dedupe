# Track Completion Audit — 0114-PdfRasterRedact

## Verdict: FAIL

## Scope Reviewed

Read `spec.md`, `plan.md`, prior r1 review, all dirty/untracked implementation files, and relevant callers/tests on `track/0114-pdf-raster-redact`. No files or Git state were modified.

## Requirement and DoD Matrix

| Requirement | Result | Evidence / Gap |
|---|---|---|
| DoD-1 raster/Image tab | Partial | PDF/image raster, page navigation, encryption handling, Rotate-90 mapping, and stub removal present. Image cap coordinates do not map back to full-resolution burn space. |
| DoD-2 draft vs burned | Partial | Geometry is separate from native CAS and QC blocks missing burns. Oversized JPEG/PNG boxes can burn the wrong region. |
| DoD-3 true burn | Partial | PDF compose uses write → parse → rewrite. Burn bookkeeping remains unbound to the source snapshot. |
| DoD-4 text-redaction honesty | Partial | QC and zero-hit UI paths exist. Metadata-mismatched PDFs larger than 64 bytes fail before classification. |
| DoD-5 schema/CI | Partial | Schema v40, appended columns, commands, and dependencies are present. Cargo gates could not execute in this environment. |
| DoD-6 recorded completion | Not scored | Explicitly out of scope for this review; no failure finding raised for registry/review completion. |

## Findings

### [P1] Burn certification is still unbound to the burned snapshot

Confidence: High  
Requirement: DoD-3; burned pointer must represent the current native, geometry, and text-redaction state.  
Location: `crates/matter-core/src/geom_redaction.rs:499-534`; `crates/dedupe-chrome/src/raster.rs:365-406`

Problem: `set_burned_native` rejects only an equal original digest. It accepts any other digest and recomputes the fingerprint after the long-running burn operation, without verifying the CAS object or expected native/geometry snapshot.

Evidence: The burn worker reads the item/fingerprint, loads and burns bytes, stores the result, then calls a setter with only `item_id` and burned digest.

Failure scenario: A concurrent native or geometry change can cause an old burn to be recorded with the new fingerprint, making `burned_native_fresh` true for bytes that do not cover the current redactions.

Correction: Pass expected native digest and source fingerprint/snapshot into an atomic conditional setter; verify the burned CAS object exists and reject snapshot mismatch.

Verification: Add concurrent native/geometry mutation and tampered-digest tests.

Deferrable: No

### [P1] Capped JPEG/PNG coordinates burn the wrong pixels

Confidence: High  
Requirement: DoD-1 and DoD-3 JPEG/PNG burn correctness.  
Location: `crates/pdf-raster/src/lib.rs:239-251`, `crates/pdf-raster/src/lib.rs:487-509`, `crates/dedupe-chrome/src/raster.rs:234-248`

Problem: The Image tab downsamples images to a 4096-pixel long side, and geometry is persisted in those capped display pixels. `burn_raster_image` then applies those coordinates directly to the original full-resolution image.

Failure scenario: A 10,000-pixel-wide JPEG with a visible secret produces a capped 4096-pixel preview. Drawing over the secret stores preview coordinates, but burn paints a different location—or a capped “full page” box leaves the remainder unredacted.

Correction: Preserve source dimensions and scale preview coordinates back to native pixel space before persistence or burn. Add an oversized-image burn oracle.

Verification: Burn an oversized JPEG/PNG and assert the visible token is gone while a separated neighbor remains.

Deferrable: No

### [P1] Burn selected set still falls back to the default set

Confidence: High  
Requirement: Burn must operate on the current selected/frozen produce set.  
Location: `crates/dedupe-chrome/ui/src/pages/produce.rs:384-389`

Problem: The QC set is preferred only when present. If the user selects “Entire review corpus” and goes directly to Burn before QC, `qc` is `None` and the code falls back to `page.ordered_ids`, which is the default set.

Failure scenario: Burn reports success while omitting items in the current selection; Finalize then encounters missing burned natives or operates on a different set.

Correction: Require a current successful QC selection for Burn, or derive the exact current selection; remove the default-set fallback.

Verification: Exercise entire-corpus selection and Burn without/after stale QC.

Deferrable: No

### [P2] Metadata-mismatched PDFs over 64 bytes fail before byte sniffing

Confidence: High  
Requirement: PDF detection must sniff native bytes when redactions exist.  
Location: `crates/matter-core/src/geom_redaction.rs:159-176`

Problem: `item_is_pdf_native` calls `get_bytes_capped(sha, 64)`. That API rejects blobs larger than 64 bytes; it does not return a 64-byte prefix. Thus a normal metadata-mismatched PDF fails with a CAS-cap error instead of being classified.

Failure scenario: A `.bin`/`application/octet-stream` PDF with text redactions cannot reach the intended `burned_native_missing`/unmapped logic and the operation hard-fails.

Correction: Add a bounded prefix-read API or read the CAS header/prefix without applying a whole-blob size cap.

Verification: Add a metadata-mismatched PDF larger than 64 bytes through QC and `resolve_native`.

Deferrable: No

### [P2] Raster truncation is not surfaced honestly

Confidence: High  
Requirement: DoD-1 cap/truncation behavior.  
Location: `crates/pdf-raster/src/lib.rs:271-282`, `crates/pdf-raster/src/lib.rs:239-251`, `crates/dedupe-chrome/ui/src/pages/review_window.rs:959-963`

Problem: JPEG/PNG raster results set `truncated`, but the UI never renders it. PDF page counts over 500 return an error and never produce a truncated result/banner.

Failure scenario: An operator sees a downscaled image as complete or receives only a raw cap error without the specified honest truncation indication.

Correction: Render a visible truncation banner and define the over-cap PDF behavior consistently.

Verification: Test oversized JPEG/PNG and over-500-page PDF UI responses.

Deferrable: No

### [P2] Keyboard-delete refresh can reintroduce stale geometry

Confidence: High  
Requirement: Image-tab generation guards must cover all raster/geometry responses.  
Location: `crates/dedupe-chrome/ui/src/pages/review_window.rs:783-794`

Problem: The keyboard Delete handler refreshes `review_geom_list` and unconditionally calls `geoms.set(g.boxes)`, unlike the guarded button-delete path.

Failure scenario: A document/page switch after the request starts allows a prior response to populate overlays for the current view.

Correction: Check returned `item_id` and captured generation before updating `geoms`.

Verification: Add a delayed delete/list response test across document and page changes.

Deferrable: No

## Completeness Sweep

- Prior Rotate-90 matrix fix is present.
- Original-native equality rejection is present, but snapshot binding remains unresolved.
- QC-first selected-set fix is present, but unsafe default fallback remains.
- JPEG/PNG long-side capping and panic containment are present, but coordinate scaling is incorrect.
- Main raster generation clearing and zero-hit messaging are present.
- No product stub copy, `process-runner` Chrome dependency, zpdf in `ui/`, GPU renderer, or production `unwrap`/`expect` found in the reviewed paths.
- zpdf 0.13.0 and CPU renderer dependencies are present in metadata/lockfile.
- No P3-only deferral is appropriate.

## Wiring and Regression Review

The core PDF raster, geometric persistence, QC, native resolution, Chrome commands, and produce integration are reachable. The P1 findings break correctness in normal concurrent workflows, oversized image workflows, and produce-set selection.

## Verification Evidence

Observed now:

- `cargo fmt --all --check` passed.
- `cargo metadata --no-deps --format-version 1` passed.
- `git diff --check` passed, with a Cargo.lock line-ending warning.
- Targeted Cargo commands were attempted but all stopped before execution because `C:\dev\Dedupe\target\debug\.cargo-lock` returned `Access is denied`:
  - `cargo test -p pdf-raster`
  - `cargo test -p matter-core --test geom_redaction`
  - `cargo test -p matter-qc burned_native`
  - `cargo test -p matter-produce burned`
  - `cargo test -p dedupe-chrome --lib raster::`
- Workspace tests, clippy, cargo deny, trunk, and release HITL were not claimed.
- Ledgerful could not open its database; AI-Brains could not run because `AI_BRAINS_KEY` is missing. Cached impact data was stale/incomplete.

## Deferred Candidates

None.

## Completion Decision

FAIL. Resolve the three P1 correctness issues and the P2 contract/state issues, then rerun the targeted and workspace gates.