# Track Completion Audit — 0094-EmbeddedMsgNestedExport

## Verdict: FAIL

Three blocking P2 findings remain. No P0/P1 findings.

## Scope Reviewed

Read-only review of the resulting worktree, track spec/plan, prior r1/r2 reviews, reader, materializer, DTO, writer, QC/oracle, tests, and deferred documentation.

## Requirement and DoD Matrix

| Area | Result |
|---|---|
| r2 aggregate BODY+HTML preflight | Met |
| r2 corrupt-recipient handling | Met at reader/DTO boundary |
| PtypObject `0x3701` writing and resolution | Met |
| Source-qualified nested-node lookup | Met |
| Child streaming via message-node API | Met |
| Method-5 probe avoidance | Met |
| Partial nested-child honesty | Partial — P2 below |
| Nested HTML error honesty | Not met — P2 below |
| QC digest compatibility | Not met — P2 below |
| DoD-5/6 | Residual accepted per instruction |

## Findings

[P2] Nested HTML read failures are silently treated as absence

Confidence: High  
Requirement: Spec §2.5 requires `body_html` preservation and honest partial extraction.  
Location: `crates/pst-reader/src/messaging/embedded.rs:424-447`  
Problem: Generic errors from HTML string/binary reads use `Err(_) => None` without setting `body_incomplete` or `body_unavailable`.  
Evidence: Plain-body errors set honesty flags at lines 401–412; HTML errors at lines 440 and 446 do not.  
Failure scenario: A nested message with a corrupt HTML property exports successfully with HTML silently removed and no fidelity signal.  
Correction: Mark the appropriate honesty flag or fail the nested extraction as unparsed. Add malformed HTML coverage.  
Verification: No dedicated generic HTML-read-error test was found.  
Deferrable: No

[P2] Soft-skipped nested child attachments remain invisible to writer fidelity reporting

Confidence: High  
Requirement: Spec §2.5 / DoD-2 require partial child-attachment failures to remain honest without inventing bytes.  
Location: `crates/pst-dedup-cli/src/pst_materializer.rs:909-912`; `crates/pst-writer/src/production.rs:501-505,1723-1754`  
Problem: `attachments_incomplete` is converted into nested `body_incomplete`, but that flag is report-only and counters are updated only for top-level messages. No attachment event or failure row represents the omitted child.  
Failure scenario: A corrupt nested child row is skipped; the nested message is written and the omission produces no nested attachment failure count or ledger event.  
Correction: Propagate nested attachment incompleteness as an attachment-fidelity event/counter, retaining available child identity, without creating placeholder bytes.  
Verification: Add a malformed child-row fixture asserting visible failure accounting.  
Deferrable: No

[P2] `content_digests_v1` and structural digest compatibility is broken

Confidence: High  
Requirement: Existing 0079 export-oracle and `content_digests_v1` clean-room behavior must remain compatible for equivalent exports.  
Location: `crates/pst-dedup-cli/src/export_oracle.rs:624-648`; `crates/pst-dedup-cli/src/unique_pst_qc.rs:1115-1118`  
Problem: The digest preimage now includes attachment method bytes and a method-5 sentinel, but the schema/version is unchanged and persisted `AttachDigestEntry` does not store method.  
Failure scenario: A parent-vs-current oracle comparison, or clean-room QC using an older `content_digests_v1`, reports false content mismatches for messages with attachments. Older method-5 entries can also be indistinguishable from empty-hash failures.  
Correction: Preserve the v1 digest algorithm and carry method-aware matching separately, or introduce an explicitly versioned digest with a legacy compatibility path.  
Verification: Add parent/current and legacy-v1 clean-room regression tests.  
Deferrable: No

## Completeness Sweep

The aggregate BODY+HTML preflight is fixed: hints are summed before subnode materialization.

The recipient fix is present: absent tables return empty successfully; present corrupt tables return `Err`, which becomes empty recipients plus `body_incomplete` in the nested export DTO.

The other r1 fixes are wired, except partial child-attachment honesty is not externally surfaced by the writer.

## Wiring and Regression Review

PtypObject writing, property-first resolution, winner-only materialization, source-qualified nested-node lookup, depth-limit mapping, serde skipping, parent-hash stability, and method-5 probe avoidance are present.

## Verification Evidence

Reported gates:

- Clippy: passed.
- Embedded writer fidelity: 6/6.
- Unique PST: 31/31.
- Budget unit test: passed.

Observed:

- `git diff --check`: passed.
- Ledgerful status: 1 pending transaction, 0 unaudited drift.
- Impact refresh could not persist under the read-only environment.

## Deferred Candidates

None. DoD-5/6 finalization residuals are accepted as directed and are not engineering findings.

## Completion Decision

FAIL. Correct the three P2 issues and add the corresponding regression tests before completion.