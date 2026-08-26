# Track Completion Audit — 0094-EmbeddedMsgNestedExport

## Verdict: FAIL

The three claimed P2 fixes are present:

- HTML read errors set `body_incomplete` ([embedded.rs:440](C:/dev/Dedupe/crates/pst-reader/src/messaging/embedded.rs:440)).
- Nested attachment metadata incompleteness propagates and emits `ATTACH_META_FAILED` without a ghost attachment ([pst_materializer.rs:909](C:/dev/Dedupe/crates/pst-dedup-cli/src/pst_materializer.rs:909), [production.rs:3807](C:/dev/Dedupe/crates/pst-writer/src/production.rs:3807)).
- Live `content_digests_v1` preimage remains method-free and method-5 skips binary reads ([export_oracle.rs:604](C:/dev/Dedupe/crates/pst-dedup-cli/src/export_oracle.rs:604)).

## Finding

[P2] Persisted clean-room digests lose the method-5 discriminator  
Confidence: High  
Requirement: Preserve `content_digests_v1` compatibility while avoiding false empty-hash failures.  
Location: [unique_pst_qc.rs:623](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:623), [unique_pst_qc.rs:1584](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1584), [unique_pst_qc.rs:1830](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_qc.rs:1830)  
Problem: `AttachDigestEntry` persists filename/size/hash but no attach method. Reloaded clean-room entries reconstruct `attach_method: None`; method-5 entries therefore enter the generic empty-hash soft-fail path.  
Failure scenario: A persisted method-5 source digest with an intentionally empty payload hash is reported as `attachment_stream_soft_fail` instead of matching the method-5 output attachment.  
Correction: Persist an optional method discriminator and add legacy handling that recognizes method-5 output before empty-hash classification. Add a clean-room legacy-digest regression test.  
Deferrable: No

## DoD Matrix

| DoD | Result |
|---|---|
| DoD-1 | Static implementation and prior reported fixture evidence present; fresh gate unavailable |
| DoD-2 | Static implementation present; no ghost attach path observed |
| DoD-3 | Static depth/budget mapping present |
| DoD-4 | Deferred docs claim closure/narrowing |
| DoD-5 | Operator smoke not verifiable; accepted residual |
| DoD-6 | Current track remains `Ready`, DoD unchecked, ledger has 1 pending transaction; accepted orchestration residual |

## Verification Evidence

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- Cargo tests/clippy: blocked before compilation by read-only access to `target\debug\.cargo-lock`.
- Ledgerful: 1 pending, 0 unaudited drift; impact risk high on dirty tree.
- AI-Brains unavailable because the vault key is missing.
- No P0/P1 or additional P2 findings found. No P3 deferral proposed.

The track should remain incomplete until the persisted-digest compatibility defect is addressed.