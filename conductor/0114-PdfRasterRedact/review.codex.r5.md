# Track Completion Audit — 0114-PdfRasterRedact

## Verdict: FAIL

## Scope Reviewed

Read all of `spec.md` and `plan.md`. Reviewed the uncommitted working tree, including untracked product files, schema/migrations, generated permissions, host/UI wiring, tests, CAS/produce paths, and configuration.

Conductor registry completion and DoD-6 recording were excluded as instructed.

## Requirement and DoD Matrix

| Requirement | Status | Evidence / Gap |
|---|---|---|
| DoD-1 Image raster/review | Partial | PDF/JPEG/PNG, navigation, encrypted/error states, rotation mapping, and generation guards are implemented. PDF long-side capping does not set the returned truncation flag. |
| DoD-2 Draft geometry separate from native | Met | Geometry persistence, overlays, stale handling, fail-closed produce behavior, and highlight separation are wired. |
| DoD-3 True burn | Met | `redact_page → write → PdfFile::parse → rewrite_pdf`; digest/fingerprint checks and CAS resolution are present. |
| DoD-4 Text-redaction honesty | Met | Unmapped text QC error, from-hits handling, quote coverage, zero-hit handling, and burn blocking are implemented. |
| DoD-5 Schema, APIs, permissions, CI | Partial / not fully independently verifiable | Schema v40, appended columns, commands, permissions, and license configuration are present. Full Cargo test/check gates were not runnable because `target\debug\.cargo-lock` returned Access Denied. |
| DoD-6 Recorded completion | Out of scope | Explicitly excluded per request. |

## Findings

[P2] PDF raster capping silently omits the required truncation signal  
Confidence: High  
Requirement: DoD-1; spec §3.3, including the 4096px cap and honest truncation banner.  
Location: [lib.rs](/C:/dev/Dedupe/crates/pdf-raster/src/lib.rs:290) and [lib.rs](/C:/dev/Dedupe/crates/pdf-raster/src/lib.rs:313)  
Problem: PDF rasterization reduces the render scale when the long side exceeds 4096px, but initializes `truncated` to `false` and returns it unchanged. The UI banner exists, but PDF previews can never activate it.  
Evidence: Image rasterization correctly returns the cap result from `cap_long_side`; the PDF path performs an equivalent cap without updating the flag.  
Failure scenario: A large PDF page requested at 150 DPI is downscaled to 4096px but is presented as uncapped, so reviewers receive no disclosure that the preview is lower resolution.  
Correction: Set `truncated = true` whenever the PDF scale is reduced by the long-side cap, and add a PDF oversized-page regression test.  
Verification: Run the targeted `pdf-raster` and Chrome raster tests after correction.  
Deferrable: No

[P2] DoD rotation acceptance is not tested through the production Chrome path  
Confidence: High  
Requirement: DoD-1 visible-token rotation fixture and §3.3 host-owned coordinate mapping.  
Location: [burn.rs](/C:/dev/Dedupe/crates/pdf-raster/tests/burn.rs:187) and [raster.rs](/C:/dev/Dedupe/crates/dedupe-chrome/src/raster.rs:300)  
Problem: The rotation test converts coordinates directly with the raster crate helpers and burns them directly. It does not call `review_geom_upsert_blocking`, then `review_burn_native_blocking`. The Chrome test coverage checks scale consistency but not rotated visible-token burn through the host command.  
Evidence: A regression in Chrome’s argument handling or host mapping could pass the existing rotation test while violating the production acceptance path.  
Failure scenario: The UI draws a box over a rotated visible token, but host conversion persists the wrong user-space rectangle and burns a neighboring region.  
Correction: Add an integration test using the Chrome upsert and burn commands against a rotated/nonzero-CropBox fixture, asserting the target token is removed and the neighbor remains.  
Verification: Run the targeted `dedupe-chrome` raster tests.  
Deferrable: No

[P3] `pdf_raster_failed` warning is rendered with blocker presentation  
Confidence: High  
Requirement: Spec §3.9: `pdf_raster_failed` is a Chrome warning extra only.  
Location: [produce.rs](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/produce.rs:451)  
Problem: The backend correctly emits `severity: "warning"` and finalization logic only blocks on extras with severity `"blocker"`, but the UI renders every extra using `class="card blocker"`.  
Evidence: [produce.rs](/C:/dev/Dedupe/crates/dedupe-chrome/src/produce.rs:244) creates the warning; [produce.rs](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/produce.rs:536) uses severity for gating but not presentation.  
Failure scenario: Counsel sees a PDF raster warning styled as a blocker and may incorrectly believe production is blocked.  
Correction: Select the warning presentation class from `e.severity`, matching the existing finding rendering.  
Verification: Exercise preflight with a PDF lacking native CAS and confirm warning styling does not block Finalize.  
Deferrable: No

## Completeness Sweep

- Prior r4 P1 geometry-write disposition is confirmed: host upsert/delete handlers surface invoke errors and skip refresh on failure.
- EML without native CAS returns `unsupported_kind` with “Not a page image (TIFF/OPT is 0115).”
- PDF raster failure warnings are emitted for missing/non-PDF native CAS, while produce remains fail-closed when burn is required.
- Fingerprints include text-redaction state; quote-unmapped, JPEG native-space, rotation, CAS sniffing, generation guards, selected QC IDs, and stale burned-native checks are present.
- No remaining `No raster yet (0114).` stub was found.
- No `process-runner` dependency was found in `dedupe-chrome`; no zpdf dependency was found in `ui/`.
- No production `unwrap`/`expect` was found in the reviewed implementation paths.
- No additional placeholder, fake-success, disconnected-command, migration, or generated-permission defect was found.

## Wiring and Regression Review

The production path is connected:

`Tauri command → join_worker → matter-core geometry/CAS → pdf-raster burn → burned CAS metadata → matter-produce resolve_native → produced native`

The burn path is fail-closed and preserves the original CAS. Geometry writes, text-redaction fingerprint invalidation, QC-required IDs, and stale-reply guards are wired.

## Verification Evidence

Observed:

- `cargo fmt --all --check` passed.
- `cargo deny check licenses` passed; only non-failing allowance/exception warnings were reported.
- `git diff --check` passed.
- `ledgerful ledger status --compact` was unavailable because its database could not be opened.
- `ledgerful scan --impact` could not write its report under the read-only environment.
- Cargo tests were not run: exclusive access to `target\debug\.cargo-lock` returned Access Denied.

Reported by orchestrator, not independently rerun:

- `cargo test -p dedupe-chrome --lib raster::`: 9 passed.
- Targeted clippy command: passed with `-D warnings`.

## Deferred Candidates

None. The P3 finding is limited but straightforward to correct; it is not an appropriate deferred item.

## Completion Decision

FAIL. Two DoD-relevant defects remain, plus one warning-presentation contract defect.