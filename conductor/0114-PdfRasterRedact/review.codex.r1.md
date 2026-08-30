# Track Completion Audit — 0114-PdfRasterRedact

## Verdict: FAIL

## Scope Reviewed

Read the complete [spec.md](C:/dev/Dedupe/conductor/0114-PdfRasterRedact/spec.md) and [plan.md](C:/dev/Dedupe/conductor/0114-PdfRasterRedact/plan.md). Audited all dirty and untracked implementation/test files on `track/0114-pdf-raster-redact`. No files or Git state were modified.

## Requirement and DoD Matrix

| Area | Result |
|---|---|
| Schema v40 and appended `ITEM_COLUMNS` | Implemented; tests present |
| Fingerprint including 0032 text state | Implemented; stale-after-text-redaction path present |
| PDF DisplayList raster and incremental rewrite burn | Implemented; compose oracle is correct |
| JPEG/PNG burn codec preservation | Implemented |
| CropBox/Rotate coordinate mapping | Fails Rotate 90 correctness |
| Draft overlays and generation guards | Partial; stale display remains possible |
| From-hits and unmapped QC | Partial; zero-hit UI is silent in cases |
| Fail-closed native resolution | Partial; metadata mismatch bypass exists |
| Burned provenance/bookkeeping | Unsafe under tampering/concurrency |
| Produce “Burn selected set” | Fails for non-default produce sets |
| Caps/truncation/panic handling | Partial; image path bypasses requirements |
| Scope boundaries / no 0117–0119 stealing | Satisfied |
| DoD-1 through DoD-5 | Partial; blockers below |
| DoD-6 recorded completion | Unmet |

## Findings

### [P1] Rotate 90 coordinate transforms are reversed

Confidence: High

Requirement: CropBox and `/Rotate` mapping must place a drawn box over the visible token and burn that token.

Location: [coords.rs](C:/dev/Dedupe/crates/pdf-raster/src/coords.rs:146), [review_window.rs](C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/review_window.rs:107)

Problem: The 90° and 270° inverse formulas are swapped relative to zpdf’s clockwise page rotation. Engine and UI use the same incorrect transform.

Evidence: Existing tests only round-trip through the same incorrect pair, so they do not verify a known visible-token position.

Failure scenario: On a rotated PDF, drawing over the visible secret creates a PDF-space rectangle on the wrong side of the page; the overlay and native burn can miss the intended content.

Correction: Implement the correct rotation matrices and add an independent Rotate90/CropBox fixture whose visible pixel location is known to contain the secret.

Verification: Burn a visible-token fixture and assert the secret is removed while the neighboring token survives.

Deferrable: No

### [P1] PDF text-redaction gating trusts metadata and can ship original PDF bytes

Confidence: High

Requirement: PDF native content with active 0032 text redactions must require geometric burn and never fall back to the original native.

Location: [geom_redaction.rs](C:/dev/Dedupe/crates/matter-core/src/geom_redaction.rs:127), [resolve.rs](C:/dev/Dedupe/crates/matter-produce/src/resolve.rs:241)

Problem: `burn_required` identifies PDFs only from path, MIME, or category metadata. A valid PDF whose metadata says `.bin` / `application/octet-stream` / non-PDF category is treated as non-PDF. `resolve_native` then copies `native_sha256`.

Failure scenario: A PDF containing a secret and an active text redaction can be exported with the original secret if its metadata is inaccurate.

Correction: Classify the actual native payload in the produce/QC gate, or conservatively fail closed whenever redactions exist but the native kind is not positively established.

Verification: Add a mismatched-metadata PDF test covering QC and `resolve_native`.

Deferrable: No

### [P1] Burned-native bookkeeping can certify the wrong bytes

Confidence: High

Requirement: The burned pointer must represent a successful burn of the current native, geometry, and 0032 text state; the original native must never be certified as fresh.

Location: [geom_redaction.rs](C:/dev/Dedupe/crates/matter-core/src/geom_redaction.rs:462), [raster.rs](C:/dev/Dedupe/crates/dedupe-chrome/src/raster.rs:365)

Problem: `set_burned_native` accepts any nonempty CAS digest, including the original native digest, and recomputes the fingerprint after the long-running burn. It does not bind the stored digest to the input native/geometry snapshot.

Failure scenario: A caller can mark original bytes as fresh. Also, a geometry/native change between loading inputs and persisting the result can cause an old burn to be recorded with the new fingerprint.

Correction: Pass and atomically verify the expected source fingerprint/native digest; reject burned digest equal to the original; verify the CAS object exists; retry or reject on snapshot changes.

Verification: Add tampered-digest and concurrent-change tests.

Deferrable: No

### [P1] “Burn selected set” burns the wrong produce set

Confidence: High

Requirement: Produce step 4 must burn the current selected produce set.

Location: [produce.rs](C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/produce.rs:384)

Problem: The UI always prefers `page.ordered_ids` when nonempty. That list comes from the default produce-page set, so it takes precedence over the current QC set even after “Entire review corpus” or profile/filter changes.

Failure scenario: The user selects a different set, clicks Burn, and the default set is burned instead. Finalize then encounters `burned_native_missing` for items in the actual selected set.

Correction: Use the current frozen QC/produce selection consistently, or disable Burn when the selection snapshot is stale.

Verification: Test altered profile and entire-corpus selection through Burn and Finalize.

Deferrable: No

### [P1] Track completion is not recorded

Confidence: High

Requirement: DoD-6 requires `review.md`, registry/status updates, deferred-row closure, and a committed Ledgerful `FEATURE` transaction.

Location: [conductor.md](C:/dev/Dedupe/conductor/conductor.md:287), [ROADMAP.md](C:/dev/Dedupe/conductor/ROADMAP.md:431), [sequencing.md](C:/dev/Dedupe/conductor/sequencing.md:130)

Problem: The track remains `In progress`/`Ready`; no `conductor/0114-PdfRasterRedact/review.md` exists; deferred rows still say `0114 Ready`. No completed ledger transaction was observable.

Correction: After fixing implementation blockers and running the required gates, complete the track’s recorded finalization workflow.

Verification: Fresh review, status registry, deferred rows, and Ledgerful transaction must all agree.

Deferrable: No

### [P2] JPEG/PNG raster paths bypass the cap and truncation contract

Confidence: High

Requirement: Enforce the 4096 long-side cap, 100 MiB/500-page policy, honest truncation UI, and panic containment.

Location: [lib.rs](C:/dev/Dedupe/crates/pdf-raster/src/lib.rs:223), [lib.rs](C:/dev/Dedupe/crates/pdf-raster/src/lib.rs:313), [review_window.rs](C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/review_window.rs:943)

Problem: JPEG/PNG inputs are decoded and re-encoded at original dimensions without long-side downscaling. PDF page-count overflow returns an error while `truncated` is always false and the UI has no truncated banner. The image branch is also outside the PDF `catch_unwind` boundary.

Failure scenario: A large image can expand substantially in memory, and oversized PDFs/images do not produce the specified honest cap/truncation behavior.

Correction: Apply bounded image decoding/downscaling, represent cap outcomes explicitly, render the required banner, and contain image-path panics.

Verification: Add oversized JPEG/PNG and over-cap PDF tests.

Deferrable: No

### [P2] Generation guards do not clear stale raster content

Confidence: High

Requirement: Changing document or page must not display a stale raster/overlay.

Location: [review_window.rs](C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/review_window.rs:366)

Problem: Responses are generation-checked, but existing `raster` and `geoms` remain populated while the new request is pending. The previous document/page can remain visible.

Failure scenario: A user changes documents or pages and sees the prior document’s image and overlays until the replacement arrives.

Correction: Clear raster, overlays, and loading state immediately when the document/page key changes; bind displayed content to that key.

Verification: Add delayed-response document/page-switch tests.

Deferrable: No

### [P2] Zero-hit “from hits” results can be silently ignored

Confidence: High

Requirement: Multi-line or otherwise unmatched searches must be reported as honest unmapped misses.

Location: [raster.rs](C:/dev/Dedupe/crates/dedupe-chrome/src/raster.rs:300), [review_window.rs](C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/review_window.rs:1050)

Problem: The host returns `hit_count`, but the UI only reports `unmapped` when the item has active text redactions and zero total geometry rows. A zero-hit query with no text redaction, or with unrelated existing geometry, produces no user-visible result.

Failure scenario: Counsel clicks “From hits,” receives no boxes, and gets no indication that the search missed.

Correction: Surface `hit_count == 0` explicitly and distinguish the current query’s mapping result from unrelated geometry.

Verification: Test zero-hit multiline queries with and without existing geometry.

Deferrable: No

## Completeness Sweep

Positive findings:

- Schema v40 and appended item columns are present.
- Fingerprint includes native, geometry, 0032 text state, and engine pin.
- Burn uses `redact_page` → incremental write → parse → `rewrite_pdf`; no `iw.document()` shortcut.
- Original native is not overwritten.
- JPEG burn preserves JPEG bytes and extension.
- Chrome commands use `join_worker`.
- No `process-runner` dependency in Chrome, no zpdf in UI, no GPU renderer, no pdfium requirement, and no production `unwrap`/`expect` found in the reviewed new paths.
- 0115 remains parked and 0116–0119 remain Proposed; no scope stealing found.

## Wiring and Regression Review

The core raster, geometry, burn, QC, and produce paths are reachable. However, the coordinate error, metadata gate, unbound burned pointer, and incorrect selected-set wiring prevent the required end-to-end security guarantee.

## Verification Evidence

Observed:

- Full dirty/untracked working tree was included.
- `git diff --check` reported only a Cargo.lock line-ending warning.
- Ledgerful status produced no visible status; impact report writing was blocked by read-only mode, and the cached impact report was stale/incomplete.
- AI-Brains could not load because the vault key was unavailable.
- No Cargo gates were run in this review.

The listed Cargo tests/checks and workspace gates remain orchestrator-reported evidence only; they are not claimed as independently executed here.

## Deferred Candidates

None. No P3-only item is appropriate while the P1 correctness, security, and completion blockers remain.

## Completion Decision

FAIL. The track is not complete. The P1 findings and DoD-6 recording must be resolved, followed by fresh targeted and workspace verification.