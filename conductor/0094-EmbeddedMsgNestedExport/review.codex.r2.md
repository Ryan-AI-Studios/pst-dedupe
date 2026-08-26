# Track Completion Audit — 0094-EmbeddedMsgNestedExport

## Verdict: FAIL

Two P2 engineering findings remain. No P0 or P1 findings.

## Scope Reviewed

Reviewed the working tree on `track/0094-EmbeddedMsgNestedExport` versus `main`, including `spec.md`, `plan.md`, implementation diffs, targeted tests, documentation, Ledgerful status, and prior review findings.

No files were modified.

## Requirement and DoD Matrix

| Area | Result |
|---|---|
| Source-qualified `(source_path, nid)` message-node lookup | Met |
| Message flags preservation | Met |
| PtypObject `0x3701` writing and property-first resolution | Met |
| Nested child streaming with `data: None` | Met |
| Method-5 probe avoidance | Met |
| Partial attachment honesty | Met |
| Nested DTO serialization/hash invariants | Met |
| DoD-1 writer fidelity | Met, based on targeted test and reported 6/6 gate |
| DoD-2 ghost/child-stream behavior | Met |
| DoD-3 depth and byte limits | **Partial — P2 below** |
| DoD-4 deferred-item updates | Met |
| DoD-5 operator note | Orchestrator residual |
| DoD-6 review/status/ledger finalization | Orchestrator residual, not an engineering P1 |

## Findings

[P2] Nested body/HTML budget is not aggregate-preflighted

Confidence: High  
Requirement: Spec §2.6 and DoD-3 require a per-nest body/HTML/child payload ceiling.  
Location: [pc.rs](C:/dev/Dedupe/crates/pst-reader/src/ltp/pc.rs:497)  
Problem: The loader compares each body or HTML subnode independently against the full 32 MiB budget, then reads all referenced subnodes. It does not sum body and HTML hints before materialization.  
Evidence: `pc.rs:497-520` checks `hint > body_byte_budget` per property. `embedded.rs:397-437` charges plain body and HTML cumulatively only after loading.  
Failure scenario: A 20 MiB plain body plus 20 MiB HTML body individually passes the 32 MiB checks, but both are materialized before the cumulative charge rejects the nest. Peak memory exceeds the specified ceiling.  
Correction: Aggregate budgeted body/HTML hints before reading any of those subnodes; also account for any child payloads loaded through the same path. Preserve the existing `ResourceLimit` mapping.  
Verification: Add or run a fixture with individually-under-budget body and HTML payloads whose combined size exceeds 32 MiB, asserting rejection before materialization.  
Deferrable: No

[P2] Corrupt nested recipient tables are silently converted to empty recipients

Confidence: High  
Requirement: Spec §2.5 requires recipients to be preserved when present and nested extraction to remain honest on partial/unreadable data.  
Location: [embedded.rs](C:/dev/Dedupe/crates/pst-reader/src/messaging/embedded.rs:451)  
Problem: Any recipient-table error is discarded by `.unwrap_or_default()`, producing a valid-looking nested message with zero recipients and no incompleteness/unparsed signal.  
Evidence: `embedded.rs:451-453`; the adjacent comment explicitly treats a corrupt recipient table as empty.  
Failure scenario: A present but malformed recipient table causes recipient data to disappear while the nested message is still exported and written as normal.  
Correction: Distinguish an absent table from a corrupt table. Propagate corruption as nested-unparsed or expose an explicit recipient-incomplete state that reaches the writer/report.  
Verification: Add a malformed-present-recipient-table fixture and assert the result is flagged or rejected rather than silently empty.  
Deferrable: No

## Completeness Sweep

All seven prior Codex r1 findings are fixed in the current tree:

- Message nodes are keyed by source path and NID, with a regression unit test.
- `message_flags` survives canonical-to-writer conversion.
- Writer fidelity resolves the nested root through property `0x3701` and asserts the NID.
- Child-stream fidelity uses `data: None` and `open_attach_data_from_message_node`.
- Soft-skipped attachments propagate to `attachments_incomplete` and `body_incomplete`.
- BODY and BODY_HTML are both included in budget preflight, though aggregate preflight remains incomplete.
- Method-5 attachments skip binary probing and deep probing.

No additional P0/P1 issue was found. Parent hash invariance, serde skipping, winner-only lazy extraction, depth mapping, and method-5 ghost avoidance are wired.

## Wiring and Regression Review

The reader, materializer, canonical DTO, and writer paths are connected correctly for the tested method-5 flow. The source-qualified lookup prevents cross-PST NID collisions. The targeted child-stream test exercises the intended write path.

The remaining defects are both nested-extraction fidelity/resource-honesty issues, not orchestration-finalization issues.

## Verification Evidence

Reported gates:

- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `writer_fidelity` embedded tests: 6/6 passed.
- `unique_pst`: 31/31 passed.
- Ledgerful status: 1 pending transaction, 0 unaudited drift.

Ledgerful impact refresh could not persist under the read-only review environment; the cached report marks the tree dirty/high risk. This is advisory and not itself a track finding.

## Deferred Candidates

None.

## Completion Decision

Engineering implementation is substantially complete and all seven r1 findings are fixed, but the two P2 findings above prevent completion. DoD-5/6 remain orchestrator residuals as specified and are not treated as engineering P1 findings.