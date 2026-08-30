# Track Completion Audit — 0114-PdfRasterRedact

## Verdict: FAIL

## Scope Reviewed

Reviewed `spec.md`, `plan.md`, tracked changes, and untracked product files in the working tree, including `pdf-raster`, Chrome raster commands/UI, matter-core geometry persistence, QC, produce resolution, and generated permissions. Conductor registry completion was excluded.

## Requirement and DoD Matrix

| Area | Result |
|---|---|
| DoD-1 raster/Image tab | Mostly pass; selection deletion bug remains |
| DoD-2 draft vs burned | Pass; r7 host-dimension fix is present |
| DoD-3 true burn/resolve | Pass by implementation and available tests |
| DoD-4 text redaction mapping | Pass by implementation |
| DoD-5 schema/CI | Partial; formatting gate fails, full Cargo gates blocked |
| DoD-6 completion metadata | Out of scope per instruction |

## Findings

[P2] Selected geometry survives document/page changes

Confidence: High

Requirement: Image-tab deletion must delete the selected box for the current review page/item.

Location: [review_window.rs](C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/review_window.rs:373), [review_window.rs](C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/review_window.rs:777), [raster.rs](C:/dev/Dedupe/crates/dedupe-chrome/src/raster.rs:334)

Problem: The document-change effect clears raster state and geometry rows but does not clear `selected_geom`. Page navigation also leaves it unchanged. The Delete handler sends only the retained geometry ID, and the host deletes that row without checking the current item, page, or generation.

Evidence: A user can select a box on document A or page 1, navigate to another document/page, then press Delete. The old, now-hidden geometry row is still deleted.

Failure scenario: This causes unintended hard deletion of a redaction geometry from a different document/page.

Correction: Clear `selected_geom` whenever `doc_id` or `raster_page_index` changes, and preferably scope deletion by item/page or validate the selected row against the current view.

Verification: Select a box, navigate documents and pages, press Delete, and confirm the previously selected row remains intact.

Deferrable: No

[P3] Workspace formatting gate fails

Confidence: High

Requirement: `cargo fmt --all --check` must pass.

Location: [raster.rs](C:/dev/Dedupe/crates/dedupe-chrome/src/raster.rs:33)

Problem: `cargo fmt --all --check` reports one formatting difference in `host_raster_dims`.

Evidence: The check proposes collapsing the multi-line `CommandError::failed(...)` call.

Correction: Run `cargo fmt --all`.

Verification: Re-run `cargo fmt --all --check`.

Deferrable: Yes

## Completeness Sweep

The prior r7 geometry issue is fixed: `host_raster_dims` rejects claimed dimensions differing from the host raster by more than 0.5 px, and both PDF and JPEG/PNG paths use host dimensions after validation.

The implementation also contains the expected v40 schema, raster engine pin, rewritten PDF burn path, burned-native fail-closed resolution, encrypted-PDF handling, stale generation checks, generated permissions, and no production `unwrap()`/`expect()` findings in the reviewed implementation paths.

DoD-6 was intentionally not evaluated.

## Verification Evidence

- Known gate supplied by orchestrator: `cargo test -p dedupe-chrome --lib raster::` — 10 passed.
- `git diff --check` — passed.
- `cargo fmt --all --check` — failed on the formatting issue above.
- Cargo test execution was blocked by `Access is denied` opening `target\debug\.cargo-lock`; not treated as an independent failure.
- `ledgerful ledger status --compact` and Ledgerful report writes were blocked by read-only database/report access.
- No files were modified.