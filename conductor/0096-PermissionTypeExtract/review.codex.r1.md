# Track Completion Audit — 0096-PermissionTypeExtract

## Verdict: FAIL

## Scope Reviewed

Reviewed `C:\dev\Dedupe` on `track/0096-PermissionTypeExtract` against `origin/main` / `e0702bc`, including all working-tree, staged, and untracked changes.

Read completely:

- `conductor/0096-PermissionTypeExtract/spec.md`
- `conductor/0096-PermissionTypeExtract/plan.md`

## Requirement and DoD Matrix

| Requirement | Status | Evidence | Gap |
|---|---|---|---|
| Reader named-property constant and NPID resolution | Met | `named_prop.rs:53-59,147-150` | None |
| Reader resolves NPID once per attachment list and calls `get_i32` | Met | `attachment.rs:466-469,561-563` | None |
| `AttachmentInfo.cloud_permission_type` | Met | `attachment.rs:74-76` | None |
| Canonical field with serde compatibility | Met | `keepset.rs:739-741` | No direct legacy JSON regression test |
| Materializer bridge | Met | `pst_materializer.rs:609-623` | None |
| Both canonical writer adapters populated | Met | `production.rs:1214-1217,1243-1246` | None |
| Cloud-pointer-only PermissionType write | Met | `production.rs:3346-3358`, `3277-3280` | QC does not honor this scope |
| Emit-only-when-used / empty NPMAP | Met | `named_prop_map.rs:148-169`; cloud-free writer test | Cloud pointer without permission is not directly asserted |
| Open-world i32 preservation | Met | No validation; writer uses `PcValue::I32` | Only value `1` is tested |
| Hasher isolation | Partial | Hash code excludes field at `hasher.rs:424-450` | Added regression test supplies identical hasher inputs |
| `AttachDetail` replaces positional tuple | Met | `export_oracle.rs:553-573` | None |
| Live-read QC and fidelity contract | Partial | `unique_pst_qc.rs:1904-1921,2011-2027`; contract row exists | QC compares non-cloud/payload-bearing attachments |
| DoD-1 | Partial | Writer round-trip test exists; live QC negative test exists | No positive canonical/materializer/unique-pst QC path |
| DoD-2 | Partial | No-invent writer logic and hash test exist | Hash test is not discriminating |
| DoD-3 | Partial | Deferred row says closed; contract row exists | QC scope defect remains |
| DoD-4 | Reported met | Orchestrator reports fmt, clippy, and focused tests passing | Not independently run due read-only constraint |
| DoD-5 | Unmet | No `review.md`; conductor remains `In Progress`; no 0096 ledger commit | Finalization still required |

## Findings

[P1] QC reports expected non-cloud PermissionType drops as defects

Confidence: High

Requirement: Spec §2.3 locks 5–6, §2.6, DoD-3.

Location: [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1904), [unique_pst_qc.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:2011)

Problem: The reader intentionally extracts PermissionType for Classic attachments (`attachment.rs:561-563`), while the writer intentionally writes it only for `is_cloud_link`. However, QC compares every `src_perm` after an attachment match, without checking whether the source row is a cloud pointer. The same comparison runs for payload-bearing attachments, whose PermissionType write-back is explicitly out of scope.

Failure scenario: A classic or payload-bearing source attachment contains PermissionType. The writer correctly drops the property, but QC sees output `None` and emits `PidNameAttachmentPermissionType` as a `Defect`.

Correction: Carry `is_cloud_link` into `AttachDetail` and the output slot, then restrict PermissionType comparison to source cloud-pointer rows.

Verification: Add regression cases for classic and payload-bearing attachments with PermissionType and assert no PermissionType defect; retain the positive cloud-pointer preservation case.

Deferrable: No

[P2] DoD-1 tests do not prove the required production bridge and the negative assertion is too broad

Confidence: High

Requirement: Spec §§2.5–2.6, DoD-1.

Location: [writer_fidelity.rs](/C:/dev/Dedupe/crates/pst-writer/tests/writer_fidelity.rs:1850), [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:4212), [unique_pst_qc_0080.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/unique_pst_qc_0080.rs:4297)

Problem: The positive writer test constructs `WriteMessage` directly, so it does not exercise reader → `AttachmentInfo` → materializer → `CanonicalAttachment` → canonical writer. The QC test also manually constructs source/output writer messages and only tests missing output. Its assertion accepts any defect:

```rust
csv.contains("PidNameAttachmentPermissionType") || report.findings.defect > 0
```

Therefore it can pass even if PermissionType-specific QC is removed.

Correction: Add a positive unique-pst/QC live-read fixture through the canonical/materializer path, assert the output property is `Some(1)`, and assert no PermissionType finding. Tighten the negative test to require the specific property and expected finding class.

Verification: Run the focused CLI QC integration test and both canonical writer adapter paths.

Deferrable: No

[P2] Hash-isolation regression test is vacuous

Confidence: High

Requirement: Spec §2.3 lock 7 and DoD-2.

Location: [keepset.rs](/C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:3622), [keepset.rs](/C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:3641)

Problem: The test creates canonical attachments with different PermissionType values, then converts both through:

```rust
AttachmentInfo::new(a.filename.clone(), a.size)
```

Both hash calls therefore receive identical `AttachmentInfo` values. The test passes regardless of whether the canonical PermissionType difference affects the real production identity path.

Correction: Add a production-path regression comparing otherwise identical source messages/PSTs differing only in PermissionType, or test the actual materialization-to-hashing boundary without discarding the differing input before the hash is computed.

Verification: Confirm both `content_hash` and `strong_content_hash` remain identical while a control attachment metadata change still changes the relevant hash.

Deferrable: No

## Completeness Sweep

No new production placeholders, stubs, fake values, `unwrap`, or `expect` paths were found in the implementation additions. Test-only `expect` usage is present as expected.

The four-crate field wiring is reachable. `unique-eml` and GUI remain unchanged as required by scope.

## Wiring and Regression Review

The main production path is wired correctly:

`pst-reader` → `AttachmentInfo` → `pst_materializer` → `CanonicalAttachment` → `from_canonical_message{,_owned}` → cloud-pointer writer → NPMAP/attach PC.

The writer correctly uses `PcValue::I32` and does not validate or narrow integer values. PermissionType is excluded from the digest preimage and attachment metadata hash inputs.

The QC path is the material defect: it lacks the source cloud-pointer discriminator despite the reader and writer explicitly distinguishing Classic from CloudLink.

## Verification Evidence

Observed during this audit:

- `git diff origin/main --check`: passed.
- Branch: `track/0096-PermissionTypeExtract`.
- Working tree remains dirty with staged, unstaged, and untracked changes.
- `ledgerful ledger status --compact`: failed — `unable to open database file`.
- `ai-brains` preflight/recall: unavailable — vault key missing.
- No files or Git state were modified.

Reported by the orchestrator, not independently run here:

- `cargo fmt --check`: passed.
- Four-crate clippy with `-D warnings`: passed.
- Focused reader, engine, writer, and CLI tests: passed.

## Deferred Candidates

None. All identified issues are DoD or correctness-related and are not eligible for `deferred.md`.

## Completion Decision

Do not mark the track complete.

Fix the QC cloud-pointer scoping defect, strengthen the end-to-end and hash-isolation tests, rerun the required gates, then have the orchestrator create `review.md`, set conductor status to `Completed`, and commit the Ledgerful `FEATURE` transaction.