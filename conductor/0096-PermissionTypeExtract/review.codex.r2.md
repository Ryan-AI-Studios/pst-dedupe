# Track Completion Audit — 0096-PermissionTypeExtract

## Verdict: FAIL

## Scope Reviewed

Working tree on `track/0096-PermissionTypeExtract` versus `origin/main` (`e0702bc`), including staged, unstaged, and untracked changes. No files or Git state were modified.

## Requirement and DoD Matrix

| Requirement | Status | Evidence / gap |
|---|---|---|
| Four-crate extraction and wiring | Met | Reader → materializer → canonical → both writer adapters are connected. |
| Cloud-pointer-only QC | Met | `should_compare_permission_type` gates both QC branches on cloud pointer plus source value. |
| Prior P1 QC scope defect | Fixed | Classic/non-cloud and payload-less paths now honor the gate. |
| DoD-1 live fidelity/QC | Partial | Direct writer and QC fixtures exist, but they bypass the canonical/materializer production bridge. |
| DoD-2 no-invent/hash isolation | Partial | Implementation is isolated, but the hash regression remains non-discriminating; no permission-specific non-cloud NPMAP test exists. |
| DoD-3 contract/deferred closure | Met engineering | Fidelity contract row and deferred closure entry are present. |
| DoD-4 gates | Reported met | `cargo fmt --all --check` independently passed. Clippy/tests were supplied as orchestrator results, not rerun. |
| DoD-5 finalization | Residual process | `review.md`, `Completed`, and ledger commit remain pending; not treated as an engineering failure per instruction. |

## Findings

[P2] DoD-1 tests still bypass the canonical/materializer bridge

Confidence: High  
Requirement: Spec §2.5; DoD-1.  
Location: [pst_materializer.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/pst_materializer.rs:620), [writer_fidelity.rs](/C:/dev/Dedupe/crates/pst-writer/tests/writer_fidelity.rs:1850), [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:4310)  
Problem: The positive writer and QC fixtures construct `WriteMessage` directly. They do not exercise reader → `AttachmentInfo` → materializer → `CanonicalAttachment` → canonical writer.  
Evidence: The production mapping exists, but the tests would remain green if that mapping were removed. The positive QC test also only asserts absence of a PermissionType finding, not that both live-read sides contain `Some(1)`.  
Failure scenario: `unique-pst` silently drops PermissionType during materialization while direct writer/QC tests continue passing.  
Correction: Add an end-to-end fixture through `unique-pst` or the real materializer path, asserting canonical `Some(1)`, output live-read `Some(1)`, and no PermissionType QC finding.  
Verification: Run the focused unique-PST/QC integration test and both canonical writer adapter paths.  
Deferrable: No

[P2] Hash-isolation regression remains non-discriminating

Confidence: High  
Requirement: Spec lock 7; DoD-2.  
Location: [keepset.rs](/C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:3620), [hasher.rs](/C:/dev/Dedupe/crates/dedup-engine/src/hasher.rs:662)  
Problem: `parent_hash_unchanged_when_cloud_permission_differs` passes the exact same `AttachmentInfo` to both hash calls. No differing PermissionType value reaches the tested boundary.  
Evidence: The size control proves size affects `content_hash`, but the test would pass even if permission were accidentally added to a future hash input or projection.  
Failure scenario: A later production mapping includes PermissionType in parent or strong hashes, while this regression test still passes.  
Correction: Compare otherwise identical production inputs differing only in canonical PermissionType, using the actual materialization/hash projection or synthetic PST scan path.  
Verification: Assert both parent and strong hashes remain equal for permission-only changes, while a filename/size control changes them.  
Deferrable: No

[P2] DoD-2 no-phantom NPMAP coverage does not test non-cloud PermissionType

Confidence: High  
Requirement: Spec §2.6 negative case; DoD-2.  
Location: [named_prop_map.rs](/C:/dev/Dedupe/crates/pst-writer/src/named_prop_map.rs:149), [named_prop_map.rs](/C:/dev/Dedupe/crates/pst-writer/src/named_prop_map.rs:399), [writer_fidelity.rs](/C:/dev/Dedupe/crates/pst-writer/tests/writer_fidelity.rs:2003), [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:4403)  
Problem: The implementation correctly rejects non-cloud rows from NPMAP planning, but no test scans a non-cloud attachment with `cloud_permission_type: Some(1)`. The classic QC test uses `write_simple_pst`, which supplies the default empty plan, so the source PST never contains the property.  
Failure scenario: PermissionType becomes plan-allowlisted for a classic attachment and emits a populated NPMAP despite being dropped from the attach row.  
Correction: Add a writer plan test with a non-cloud attachment carrying `Some(1)` and assert the PermissionType NPID is absent and the NPMAP remains empty.  
Verification: Run named-prop and writer fidelity tests.  
Deferrable: No

## Completeness Sweep

No new production placeholders, stubs, fake success paths, or unsupported `unwrap`/`expect` additions were found in the changed implementation.

## Wiring and Regression Review

The implementation itself is sound:

- Reader resolves the named property once per attachment list and reads it as `i32`.
- Materializer maps the value into canonical state.
- Both canonical writer adapters preserve it.
- Writer emission is reached only through the cloud-pointer path.
- QC now compares PermissionType only for cloud-pointer source rows with a source value.
- Hash metadata and the live digest preimage exclude PermissionType.

## Verification Evidence

Observed:

- `git diff origin/main --check`: passed.
- `cargo fmt --all --check`: passed.
- Branch and working-tree scope confirmed.
- Ledgerful status/doctor unavailable: local database could not be opened.
- ai-brains unavailable: vault key missing.

Reported by orchestrator:

- Four-crate clippy with `-D warnings`: passed.
- Focused QC, cloud/classic, and writer tests: passed.

## Deferred Candidates

None. The P2 findings affect DoD evidence and are not deferrable.

## Completion Decision

Do not mark the track complete yet. The prior QC scope defect is fixed, but the end-to-end DoD-1 proof, discriminating hash regression, and non-cloud NPMAP negative test still need strengthening. DoD-5 remains an orchestrator finalization step after engineering review passes.