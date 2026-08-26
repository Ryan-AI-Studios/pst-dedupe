# Track Completion Audit — 0096-PermissionTypeExtract

## Verdict: FAIL

## Scope Reviewed

Reviewed the dirty working tree on `track/0096-PermissionTypeExtract`, including staged, unstaged, and untracked changes, against the complete `spec.md` and `plan.md`.

## Requirement and DoD Matrix

| Requirement | Status | Evidence |
|---|---|---|
| Reader named-property extraction | Met | `named_prop.rs`, `attachment.rs`; `get_i32` and one-time NPID resolution |
| Canonical/materializer/writer wiring | Met in implementation | `pst_materializer.rs:620`; both canonical adapters copy the field |
| Cloud-only write and NPMAP planning | Met | `is_cloud_link` gate; `scan_ignores_non_cloud_attaches` passes |
| QC `AttachDetail` and cloud gate | Met | Both QC branches use `should_compare_permission_type` |
| Fidelity contract | Met | `PidNameAttachmentPermissionType` marked `Preserved` |
| Hash isolation | Met | Permission-only test plus filename:size control pass |
| DoD-1 | Partial | Writer round-trip and QC fixtures exist, but do not exercise reader → materializer → canonical → production writer |
| DoD-2 | Met | No-invent behavior, non-cloud empty plan, hash isolation |
| DoD-3 | Met engineering | Deferred entry is closed; contract row exists |
| DoD-4 | Reported met | fmt observed passing; clippy/focused tests reported passing |
| DoD-5 | Residual orchestration | Correctly excluded from engineering verdict per instruction |

## Findings

[P2] DoD-1 still lacks production reader/materializer path coverage  
Confidence: High  
Requirement: Spec §2.5–§2.6; DoD-1  
Location: `crates/pst-dedup-cli/src/pst_materializer.rs:620`, `crates/pst-writer/tests/writer_fidelity.rs:1851`, `crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:4310`  
Problem: The new writer test starts from a manually constructed `CanonicalAttachment`. The QC tests manually construct `WriteMessage` objects on both sides. No PermissionType test invokes `PstMaterializer` on a reader-produced `AttachmentInfo`, then passes the resulting canonical message through the production unique-PST writer path.  
Evidence: The mapping at `pst_materializer.rs:620` is correct, but removing that mapping would not be detected by the new tests. Production `unique-pst` uses `from_canonical_message_owned` at `unique_pst_cmd.rs:3401`, which is also not covered by this fixture.  
Failure scenario: A source PST contains PermissionType, the reader extracts it, but materialization drops it; direct writer/QC fixtures remain green while the real export loses the property.  
Correction: Add an end-to-end fixture that materializes a writer-generated source PST, asserts canonical `cloud_permission_type == Some(1)`, writes through the production/owned adapter, live-reads output `Some(1)`, and asserts no PermissionType QC finding.  
Verification: Run the focused CLI QC/unique-PST tests and the writer fidelity test.  
Deferrable: No

## Completeness Sweep

No new production placeholders, stubs, silent fallbacks, or unsupported error paths were found. The prior QC-scope defect, hash-test defect, and non-cloud NPMAP-test defect are fixed.

## Verification Evidence

Observed:

- `cargo fmt --all --check`: passed.
- `git diff HEAD --check`: passed.
- Hash-isolation test: passed.
- `attachment_meta_strings` filename:size test: passed.
- Exact name/GUID test: passed.
- Non-cloud NPMAP plan test: passed.
- QC gate and fidelity-contract unit tests: passed.

Reported by the orchestrator/user:

- Four-crate clippy with `-D warnings`: passed.
- Focused tests: passed.

Unavailable under read-only sandbox:

- Ledgerful status/doctor/verify: database/report access denied.
- Integration tests requiring temp PST creation: OS temp-directory writes denied.

## Deferred Candidates

None. The remaining P2 is not deferrable.

## Completion Decision

The implementation is largely correct and all listed r2 fixes are present, but the prior DoD-1 production-path evidence gap remains. Do not mark the track complete until the reader/materializer/owned-writer end-to-end test is added and verified.