# Track Completion Audit — 0114-PdfRasterRedact

## Verdict: FAIL

## Scope Reviewed

Read-only review of the working tree on `track/0114-pdf-raster-redact`, including unstaged and untracked product files. Read all of `spec.md` and `plan.md`. Conductor registry completion was excluded as instructed.

## Requirement and DoD Matrix

| Area | Result |
|---|---|
| Schema v40, geometry persistence, append-only item columns | Met |
| PDF/image rasterization, caps, coordinate transforms, rotation | Met |
| Draft geometry, hit mapping, generation guards | Partial |
| Native burn, rewrite, provenance, fresh-fingerprint enforcement | Met |
| Chrome Image tab, Produce Burn, QC counters and blockers | Partial |
| DoD-6 completion recording | Out of scope per instruction |

## Findings

### [P1] Geometry write failures are silently discarded

Confidence: High  
Requirement: Spec §3.4, §3.11; DoD-2  
Location: `crates/dedupe-chrome/ui/src/pages/review_window.rs:1009-1026, 1139-1160`

Problem: Full-page and drawn geometry calls discard the `tauri_invoke` result with `let _ = ...`. Backend failures are neither shown nor retained as pending errors.

Evidence: The UI reloads the geometry list after the ignored write, so a failed write can look like an empty or unchanged overlay.

Failure scenario: A database, generation, validation, or persistence failure causes the intended redaction box not to be stored. The user receives no error; production then sees no geometry and may package the original native without burning.

Correction: Handle geometry mutation errors explicitly, distinguish stale-generation responses from real failures, surface failures to the user, and prevent completion/production from proceeding with an unacknowledged failed mutation.

Verification: Add UI/integration coverage for failed draw and full-page upsert paths and confirm the error is visible and production remains safely blocked.

Deferrable: No

### [P2] EML items without native CAS bytes do not receive the required honest empty state

Confidence: High  
Requirement: DoD-1; unsupported-kind behavior  
Location: `crates/dedupe-chrome/src/raster.rs:27-37, 68-76`

Problem: Raster review requires `native_sha256` before classifying the item. Valid synthetic-EML items without native bytes therefore return `item has no native_sha256` instead of the required “Not a page image (TIFF/OPT is 0115)” empty state.

Evidence: `matter-produce` explicitly supports export-only synthetic EML when native bytes are missing.

Failure scenario: Opening Image view for a valid no-native EML produces a generic command error rather than an honest unsupported-page response.

Correction: Classify the item from metadata/category before requiring native CAS bytes, and map non-page kinds—including no-native EML—to the specified unsupported empty state.

Verification: Add a Chrome raster test for a synthetic EML with no `native_sha256`.

Deferrable: No

### [P2] Required `pdf_raster_failed` Chrome warning extra is not wired

Confidence: High  
Requirement: Spec §3.8  
Location: `crates/pdf-raster/src/error.rs:49`; `crates/dedupe-chrome/src/produce.rs:303-331`

Problem: `pdf-raster` exposes the `pdf_raster_failed` kind, but no Chrome warning-extra path emits or records it. Raster failures are only displayed as UI errors.

Evidence: Repository search finds `pdf_raster_failed` only in the raster error mapping; `chrome_extras` contains no raster-failure handling.

Failure scenario: A PDF raster failure is not represented in the required Chrome warning channel, so downstream QC/production diagnostics cannot distinguish it from an ordinary UI error.

Correction: Wire raster failure state into the required Chrome warning-extra mechanism while retaining the existing burn/production fail-closed behavior.

Verification: Add a failure-path test asserting a `pdf_raster_failed` warning extra is emitted.

Deferrable: No

## Completeness Sweep

The following were confirmed present in the resulting implementation:

- Schema v40 and append-only `ITEM_COLUMNS` additions.
- Host-authoritative raster dimensions and coordinate conversion.
- Correct 90/270 rotation mapping.
- PDF sniffing from CAS bytes.
- 4096 long-side image cap and native-space JPEG/PNG persistence.
- Burn provenance checks, CAS existence, digest inequality, and expected-fingerprint validation.
- PDF text-redaction fail-closed behavior for unmapped quotes.
- QC-selected burn IDs; no page-order fallback.
- Generation checks for raster and geometry responses.
- Zero-hit “from hits” handling.
- No PDF bytes or `pdf.js` path in the WebView.
- No direct `process-runner`, GPU renderer, or `iw.document()` usage in the reviewed paths.
- No production `unwrap()`/`expect()` observed in the audited new Rust paths.

The previously identified text/fingerprint, dimension-scaling, and QC-counter issues are independently fixed in the current source, with corresponding regression tests present.

## Wiring and Regression Review

Core raster → geometry → burn → CAS → produce wiring is reachable and substantively implemented. However, the silent geometry mutation failures create a fail-open UI path, and the required raster warning signal is incomplete.

## Verification Evidence

Observed:

- `cargo fmt --all --check` passed.
- `git diff --check` passed.
- `cargo deny check licenses` passed with existing unmatched-license warnings.
- Targeted Cargo tests could not start because `target\debug\.cargo-lock` returned `Access is denied (os error 5)`.
- Ledgerful status/impact commands were unavailable under the read-only environment.

Orchestrator-reported, not independently rerun:

- `cargo test -p dedupe-chrome --lib raster::` — 7 passed.
- `cargo test -p pdf-raster quote_unmapped`.
- `cargo test -p matter-core --test geom_redaction fingerprint_mismatch`.

No broader workspace test, clippy, or full deny result is claimed.

## Deferred Candidates

None. All findings require correction before completion.

## Completion Decision

FAIL. The track is not engineering-complete because geometry persistence can fail silently and still permit production, and the specified `pdf_raster_failed` Chrome warning path is missing.