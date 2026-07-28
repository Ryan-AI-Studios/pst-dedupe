# Track Completion Audit — 0074-DeepAttachPreflightFidelity

## Verdict: FAIL

## Scope Reviewed

Read the full `spec.md`, `plan.md`, all prior reviews, current dirty implementation, untracked `attach_probe.rs`, shared scan/unique-pst/materializer paths, docs, tests, Git state, and Ledgerful state.

## Requirement and DoD Matrix

| DoD | Status | Evidence |
|---|---|---|
| 1–5 | Met | Probe engine, winner path, scan flag, reason codes, preflight fields |
| 6 — Keep-set | Partial | Peer-cap semantics still allow an unprobed clean peer to win |
| 7–11 | Met, with allowed residuals | Stream flags, budgets, cancel/progress, parents-only, honesty, timeout |
| 12 — Peer cap | Partial | Cap counter/budget bound works; winner fidelity does not |
| 13 — Cache | Met | Level, mtime, and source-file size included |
| 14 — Tests | Met for synthetic unit coverage | Production E2E matrix remains a permitted P3 residual |
| 15 — Docs | Met | CLI flags, budgets, residual ledger guidance |
| 16 — Recorded | Pending orchestrator closeout | No canonical `review.md`, Completed registry entry, D-0074 records, or ledger commit |

## Findings

### [P1] Peer cap leaves unprobed peers eligible as clean winners

Confidence: High  
Requirement: §3.7.1, DoD-6, DoD-12  
Location: [attach_probe.rs:1174](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:1174), [attach_probe.rs:1272](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:1272), [keepset.rs:507](C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:507), [keepset.rs:748](C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:748)

Problem: After N failed probes, the implementation records `peer_probe_capped_groups` but does not mark or otherwise constrain the remaining unprobed candidates. Resolver ranking then treats those candidates as clean and prefers them over the degraded probed peers.

Failure scenario: With cap 3, the first three peers fail and a fourth unprobed peer remains clean. `resolve_groups` can select the fourth peer, causing the supposedly bounded preflight to export an unverified candidate and defer the same attachment failure to materialization/export.

Correction: Carry capped-group state into resolution so an unprobed candidate cannot outrank the capped provisional winner; preserve the best-effort winner as degraded per the locked rule.

Verification: Add a synthetic group with N failed peers plus a clean-looking peer beyond the cap; assert the cap is preserved and the unprobed peer cannot silently become the clean winner.

Deferrable: No

## Prior Finding Verification

Fixed:

- Strict scan row/result rebuilding.
- Strict per-file message and recoverable decrements.
- Unique-pst per-file duplicate and degraded recomputation.
- Full-level timeout budget charging.
- Source PST size in cache identity.
- Scan progress and cancellation-before-probe reporting.
- Stream-open reason mapping.
- Exact-N peer-cap tally.

## Completeness Sweep

No new blocking placeholders, source mutation, ScanPST execution, per-page CRC logging, or unbounded probe buffer were found. The materializer’s small-payload `read_to_end` remains capped; its unbounded handle maps are the allowed `mat-lru` residual.

## Deferred Candidates

Only after the P1 is fixed:

- `D-0074-gui`
- `D-0074-mat-lru`
- `D-0074-cache-share`
- `D-0074-crc-fixture`
- `D-0074-timeout-join`
- Production-path E2E fixture matrix, with current synthetic unit coverage accepted for DoD-14

These are not currently recorded; the reviewer is read-only.

## Verification Evidence

Observed:

- `cargo fmt --all --check` passed.
- `cargo metadata --no-deps --format-version 1` passed.
- `git diff --check main` passed.
- Ledgerful status: 1 pending transaction, 0 unaudited drift.
- Cargo tests were blocked by read-only access to `target\debug\.cargo-build-lock`.
- `ledgerful verify` passed formatting but could not complete clippy/tests for the same environment restriction.

Reported by orchestrator:

- dedup-engine lib: 94 tests passed.
- pst-dedup-cli lib: 82+ tests passed.
- Touched-crate clippy passed.

## Completion Decision

tokens usedThe final3 P1 is fixed, but the peer-cap winner-selection defect is a new core fidelity failure. The track remains **FAIL** pending that correction and targeted regression coverage.

341,417

