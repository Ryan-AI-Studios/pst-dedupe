# Track Completion Audit — 0114-PdfRasterRedact

## Verdict: FAIL

## Scope Reviewed

Read all of `spec.md` and `plan.md`. Reviewed scoped tracked, unstaged, and untracked product files, excluding conductor registry completion and DoD-6 recording.

## Requirement and DoD Matrix

| Area | Result |
|---|---|
| Raster preview, caps, cache state, navigation | Implemented; r6 truncated-cache fix present |
| Geometry CRUD, transforms, generation guards | Partial; host dimensions remain client-controlled |
| True PDF/image burn and burned-native resolution | Implemented |
| Text-redaction honesty and fail-closed production | Implemented |
| Schema v40, permissions, dependency boundaries | Implemented |
| DoD-1 | Fails host-owned coordinate authority requirement |
| DoD-2 | Met by code/tests |
| DoD-3 | Met by code/tests |
| DoD-4 | Met by code/tests |
| DoD-5 | Code present; full gates not independently runnable |
| DoD-6 | Out of scope per instruction |

## Findings

[P2] Geometry upsert trusts client-supplied raster dimensions

Confidence: High

Requirement: Spec §3.3 and DoD-1 require host-owned pixel-to-PDF/image coordinate conversion.

Location: `crates\dedupe-chrome\src\raster.rs:243`, `crates\dedupe-chrome\src\raster.rs:274`

Problem: The host rerasterizes the page and obtains authoritative dimensions, but scales the incoming pixel rectangle using `args.raster_width` and `args.raster_height`. A stale or malformed IPC request can therefore persist incorrect PDF or native-image coordinates.

Evidence: PDF conversion uses `cw/ch` from the request at lines 243–251. JPEG/PNG conversion does the same at lines 274–281. The existing scale test only proves proportionally changed coordinates produce the same result; it does not reject unchanged coordinates with falsified dimensions.

Failure scenario: A drag generated from an old preview, or an IPC caller supplying mismatched dimensions, causes the persisted rectangle to shift or resize. Burn can then miss the visible secret or redact a neighboring region.

Correction: Derive the coordinate dimensions exclusively from the host raster result, or reject requests whose claimed dimensions do not exactly match the current host raster dimensions. Add a regression test with deliberately mismatched dimensions on a rotated/CropBox PDF.

Verification: Re-run the host command test and the targeted PDF burn tests.

Deferrable: No

## Completeness Sweep

The r6 LRU fix is present: `CacheEntry` stores and clones `truncated`, cache hits return it, and `pdf_long_side_cap_survives_lru_hit` exists.

The prior text-redaction fingerprint issue, selected-set counting issue, stale-reply guards, CAS-prefix PDF detection, fail-closed burned-native resolution, warning severity rendering, true PDF rewrite pipeline, and codec-preserving image burn remain addressed.

No remaining raster stub, `iw.document()` rewrite shortcut, Chrome `process-runner` dependency, GPU renderer, or zpdf dependency in `ui/` was found. PDFs over 500 pages were not treated as a failure.

## Wiring and Regression Review

The end-to-end path is wired through raster commands, geometry persistence, burn, CAS resolution, QC, and production. The remaining defect is at the geometry command boundary: host-side transformation is implemented, but its scale inputs remain caller-controlled.

## Verification Evidence

Observed:

- `cargo fmt --all --check` — passed.
- `git diff --check` — passed.
- `cargo metadata --no-deps` — passed; workspace and dependency wiring confirmed.
- Targeted Cargo test attempt — blocked before execution by access denied opening `target\debug\.cargo-lock`.
- Orchestrator-reported `cargo test -p pdf-raster --test burn pdf_long_side` — 2 passed, not independently reproduced.
- Ledgerful and ai-brains checks were unavailable due database/key configuration; no files were modified.

## Deferred Candidates

None. The remaining P2 is an engineering defect, not a qualifying deferred P3.

## Completion Decision

FAIL. Fix host-side raster-dimension authority and add the mismatched-dimension regression test before completion.