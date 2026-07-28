# Track Completion Audit — 0074-DeepAttachPreflightFidelity

## Verdict: FAIL

## Scope Reviewed

Reviewed the full spec/plan, all three prior reviews, current uncommitted diff, CLI scan and unique-pst wiring, probe engine, materializer cache bridge, docs, tests, governance, and Ledgerful state.

Branch and `main` both point to `844ec5a`; implementation remains uncommitted. Unrelated 0075 changes were excluded except shared-file interactions.

## Requirement and DoD Matrix

| DoD | Status | Evidence |
|---|---|---|
| DoD-1 Probe engine | Met | L1–L3 chunked probe, hard budgets, Full timeout charge |
| DoD-2 Winner path | Met | Winner probe precedes resolve; cache reaches materializer |
| DoD-3 Scan path | Met | Flag, budgets, peer cap, progress, cancellation |
| DoD-4 Reasons | Met | Additive `ATTACH_*` codes and mappings |
| DoD-5 Preflight | Partial | Aggregate report is reconciled; per-file output remains stale |
| DoD-6 Keep-set fidelity | Met | Failures degrade candidates; clean peers rank first |
| DoD-7 Stream flags | Met | Cached failures set `stream_available=false` |
| DoD-8 Safety | Met with residuals | Fixed discard buffer and probe LRU; materializer LRU remains deferred |
| DoD-9 `parents_only` | Met | Probe skipped |
| DoD-10 Honesty | Met | Coverage, cancellation, residual-risk, and ScanPST guidance documented |
| DoD-11 Timeout | Met with residual | Budget is charged; worker joining remains deferred |
| DoD-12 Peer cap | Met | Exact-N cap accounting fixed |
| DoD-13 Cache | Met | Level, mtime, and source PST size included |
| DoD-14 Tests | Partial | Added unit tests, but locked production paths remain untested |
| DoD-15 Docs | Met | Scan/unique-pst flags, budgets, 0073 interaction documented |
| DoD-16 Recorded | Pending | No canonical `review.md`, registry remains Ready, one pending Ledgerful transaction |

## Findings

### [P1] Post-probe per-file statistics remain inconsistent

Confidence: High  
Requirement: DoD-3, DoD-5, DoD-6  
Location: [scan.rs](C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:233), [unique_pst_cmd.rs](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:1195)

Problem: `apply_strict_probe_skips_to_file_stats` updates only `skipped`, `recoverable_messages`, and status. It does not reconcile `messages` or `duplicates`. Those fields were populated before probing. In unique-pst best-effort mode, aggregate degradation is recomputed, but `summary.files[*].degraded_messages`, reason maps, and status are not updated.

Failure scenario: A strict probe skip causes aggregate `total_messages`/`duplicates` to be rebuilt, while the affected file still reports the pre-probe message and duplicate counts. In best-effort mode, aggregate degradation can be nonzero while the file still reports `opened` and zero degraded messages.

Correction: Centralize post-probe reconciliation from surviving candidates and apply it to all per-file message, duplicate, skipped, degraded, reason, and status fields in both scan and unique-pst paths.

Verification: Fixture/CLI JSON tests covering strict winner removal and best-effort attach degradation; assert per-file sums and statuses match aggregate output.

Deferrable: No

### [P2] Locked acceptance cases still lack production-path coverage

Confidence: High  
Requirement: DoD-14, §3.11  
Location: [attach_probe.rs](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:1433), [attach_probe.rs](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:1878)

Problem: Timeout tests use standalone channel/deadline simulations rather than the production timed probe. LRU tests cover failed opens, not valid eviction/live-handle limits. No integration tests exercise deep-probe scan JSON/CSV reconciliation, unique-pst materializer cache propagation, or strict production output.

Correction: Add fixture-backed or production-compatible tests for the locked matrix, especially actual timed probing, valid LRU eviction, scan output reconciliation, and unique-pst cache/stream flags.

Verification: Run the complete §3.11 matrix through scan and unique-pst paths.

Deferrable: No

## Prior Finding Verification

Fixed: strict `DedupResult` rebuilding, Full timeout budget charging, source PST size in cache identity, scan progress, cancellation-before-probe reporting, exact-N peer cap, and stream-open reason mapping.

Partially fixed: unique-pst per-file reconciliation.

Not fixed: production-path acceptance coverage.

## Verification Evidence

Observed:

- `cargo fmt --all --check` passed.
- `git diff --check main` passed.
- `cargo metadata --no-deps --format-version 1` passed.
- Targeted Cargo tests were blocked by read-only access to `C:\dev\Dedupe\target\debug\.cargo-build-lock`.
- `ledgerful verify` failed its clippy/test steps for the same environment restriction.
- Ledgerful status: 1 pending transaction, 0 unaudited drift.
- Impact scan was unavailable because Ledgerful could not open its database; cached impact is high-risk and stale for the dirty tree.
- Orchestrator-reported gates remain unindependently observed: dedup-engine 94, CLI 81, touched clippy, GUI check.

## Deferred Candidates

Only after the blocking findings are fixed: `D-0074-gui`, `D-0074-mat-lru`, `D-0074-cache-share`, `D-0074-crc-fixture` narrowly for real CRC fixture evidence, and `D-0074-timeout-join` narrowly for worker joining.

No files or Git state were modified.