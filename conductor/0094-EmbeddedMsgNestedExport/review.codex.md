# Track Completion Audit — 0094-EmbeddedMsgNestedExport

## Verdict: FAIL

The main nested-export path is implemented, but multi-source nested streaming can select the wrong PST root. Additional fidelity and acceptance-test gaps remain.

## Scope Reviewed

- Branch: `track/0094-EmbeddedMsgNestedExport`
- Base: `main` / `5351b84`
- Working tree, staged changes, and untracked files
- Full `spec.md` and `plan.md`
- Reader, materializer, writer, dedup DTO, QC, EML, tests, docs, and deferred records
- Read-only review; no files or Git state modified
- Ledgerful impact report: high-risk working tree; no persisted test mapping

## Requirement and DoD Matrix

| Item | Status | Evidence / Gap |
|---|---|---|
| Full method-5 nested DTO and winner-only extraction | Partial | Wired through `materialize_nested_for_winner`; no production-path integration test. |
| PtypObject `0x3701` write and property-first resolve | Partial | Code is present; test permits scan fallback and does not prove property resolution. |
| Nested child streaming | Partial | Correct helper is wired, but registry is not source-qualified; test buffers child bytes directly. |
| Depth, byte budgets, honesty reasons | Partial | Depth mapping exists; HTML budget preflight and failure honesty are incomplete. |
| Serde skip, parent hash stability, unique-eml behavior | Met | `serde(skip)` and hash regression are present; EML path remains unchanged. |
| DoD-1 | Partial | Writer round-trip passes reported gates, but property-primary resolution and source extraction are not independently proven. |
| DoD-2 | Partial | Fail-closed writer behavior exists; unreadable nested extraction and real nested source streaming are not covered end to end. |
| DoD-3 | Partial | Depth flag maps correctly in writer tests; extract-side depth/byte behavior lacks production integration coverage. |
| DoD-4 | Met | `D-0069` is documented closed; `D-0067` is narrowed with explicit residuals. |
| DoD-5 | Not verifiable | INC0102784 re-smoke evidence is not present in the track review artifact. Finalize residual. |
| DoD-6 | Open | `review.md`, Completed status, and ledger commit remain finalize steps. |

## Findings

[P1] Nested child streams are keyed only by NID across source PSTs

Confidence: High  
Requirement: DoD-2; §2.8; multi-source export correctness  
Location: [pst_materializer.rs](C:/dev/Dedupe/crates/pst-dedup-cli/src/pst_materializer.rs:677)  
Problem: `message_nodes` is `HashMap<u64, MessageNodeRef>`. Registration and lookup ignore `source_path`. PST NIDs are only unique within a store.  
Evidence: `register_message_node` stores by `node.nid.0`; `open_attachment_data_reader` looks up only by `parent.nid`.  
Failure scenario: Two input PSTs contain nested messages with the same NID. The later registration overwrites the earlier root, so a child attachment can be streamed from the wrong PST or fail.  
Correction: Key registrations by `(source_path, nid)` and use both values for lookup.  
Verification: Add a two-source fixture with deliberately colliding nested NIDs and distinct child payloads.  
Deferrable: No

[P2] Readable nested message flags are extracted but discarded

Confidence: High  
Requirement: §2.5 `message_flags` BestEffort preservation  
Location: [production.rs](C:/dev/Dedupe/crates/pst-writer/src/production.rs:1137)  
Problem: `NestedCanonicalMessage.message_flags` is populated by the reader but `WriteMessage` has no corresponding field. The writer always synthesizes `MSGFLAG_READ`.  
Failure scenario: A nested source message with readable flags is exported with different flags, losing source fidelity.  
Correction: Carry optional flags through the writer DTO and preserve them, with an explicit safe default only when absent.  
Verification: Round-trip nested messages with distinct readable flag values.  
Deferrable: No

[P2] DoD-1 does not prove `0x3701` property-primary resolution

Confidence: High  
Requirement: DoD-1 and §2.4  
Location: [writer_fidelity.rs](C:/dev/Dedupe/crates/pst-writer/tests/writer_fidelity.rs:603)  
Problem: The test checks that an object exists, then calls a resolver that may silently use the scan fallback. It never asserts that the resolved NID equals the `0x3701` object NID.  
Correction: Assert `nested_root.nid == object_nid`; add a separate legacy fixture proving fallback behavior.  
Verification: Break property resolution while retaining the scan path; the test should fail.  
Deferrable: No

[P2] Nested child-stream test does not exercise source streaming

Confidence: High  
Requirement: DoD-2; §2.8  
Location: [writer_fidelity.rs](C:/dev/Dedupe/crates/pst-writer/tests/writer_fidelity.rs:1523)  
Problem: The test supplies `data: Some(b"ABCD")`, so `AttachStreamSource` and `open_attach_data_from_message_node` are never used.  
Failure scenario: A regression to NBT-based lookup or broken nested-root registration would still pass.  
Correction: Build/open a source PST, materialize a nested message with `data: None`, run the production stream source, and assert the child bytes.  
Verification: Require the real nested path and assert no `NodeNotFound`.  
Deferrable: No

[P2] Partial nested attachment failures are silently dropped

Confidence: High  
Requirement: §2.5 and product lock 1  
Location: [embedded.rs](C:/dev/Dedupe/crates/pst-reader/src/messaging/embedded.rs:438)  
Problem: Recipient-table errors become `unwrap_or_default()`, and attachment-row errors are skipped under `fail_on_row_error=false`. No nested fidelity marker records the omitted child.  
Failure scenario: A malformed child attachment disappears from the nested DTO without an attachment failure event or explicit honesty state.  
Correction: Preserve per-child failure metadata or fail the nested extract with a distinct, writer-visible fidelity result.  
Verification: Add a malformed child-PC/table fixture and assert an explicit residual.  
Deferrable: No

[P2] HTML payloads are charged after potentially being materialized over budget

Confidence: High  
Requirement: §2.6 32 MiB per-nest ceiling  
Location: [pc.rs](C:/dev/Dedupe/crates/pst-reader/src/ltp/pc.rs:485)  
Problem: The budgeted loader preflights only the body property, while loading all referenced subnodes—including HTML—before `read_export_from_message_node` charges HTML bytes.  
Failure scenario: An oversized HTML subnode is read into memory before being rejected as `ATTACH_DEPTH_LIMIT`.  
Correction: Preflight HTML subnode sizes before loading, or make subnode loading budget-aware for every body property.  
Verification: Add an oversized HTML XBLOCK fixture.  
Deferrable: No

[P2] Small valid method-5 attachments are misclassified as incomplete

Confidence: High  
Requirement: Method-5-only handling; winner selection honesty  
Location: [pst_materializer.rs](C:/dev/Dedupe/crates/pst-dedup-cli/src/pst_materializer.rs:549)  
Problem: For small attachments, materialization calls binary `open_attachment_data` before nested extraction. Method-5 correctly fails that binary path, setting `stream_available=false` and an attach failure reason. Nested extraction runs later, after `finalize_with_materialize_opts` has already evaluated `is_attach_incomplete`.  
Failure scenario: With `--promote-on-attach-fail`, a valid small nested message may be incorrectly treated as an incomplete peer.  
Correction: Skip binary probing for method-5 attachments; classify them through nested extraction only.  
Verification: Add a small valid method-5 winner and assert no false promotion/degraded attach state.  
Deferrable: No

## Completeness Sweep

- No new production stubs, fake success paths, or placeholder implementations found.
- New nested DTO fields are correctly skipped from serde.
- Unique-EML does not consume the nested DTO.
- Writer-side anti-ghost and fail-closed paths are present.
- `unwrap`/`expect` matches inspected in affected source files are test-only.
- No required migration or generated schema change was identified.

## Wiring and Regression Review

The primary path is reachable:

`unique-pst winner → materialize_nested_for_winner → property-first nested resolve → full nested extract → NestedCanonicalMessage → writer adapter → method-5 subnode + PtypObject`

The source-qualified stream-key defect breaks nested child fidelity for multi-source exports. The writer tests also do not exercise the actual source materializer/stream path.

## Verification Evidence

Reported by the orchestrator:

- Four-crate `cargo check`: passed
- Four-crate clippy with `-D warnings`: passed
- Writer embedded fidelity tests: 6/6
- `unique_pst`: 31/31
- Parent-hash regression: passed
- Serde-skip regression: passed

Not independently rerun during this read-only review:

- Full Cargo verification
- Production `materialize_nested_for_winner` integration
- Multi-source collision case
- INC0102784 re-smoke
- Outlook/client smoke

## Deferred Candidates

None. The findings are substantive P1/P2 issues, not deferrable P3 items.

## Completion Decision

Engineering completion is not established because the multi-source nested stream registry can misroute child evidence, and several required fidelity/test guarantees remain incomplete.

DoD-5 and DoD-6 are also still open as the explicitly noted finalize residuals: no `review.md`, no Completed conductor status, no ledger FEATURE commit, and no recorded operator re-smoke evidence.