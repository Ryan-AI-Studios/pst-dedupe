# Track Completion Audit — 0074-DeepAttachPreflightFidelity

## Verdict: FAIL

No P0 findings. P1/P2 correctness, budget, reporting, and verification gaps remain.

## Scope Reviewed

Read all of `spec.md` and `plan.md`, including §§2.2, 3.1–3.12, risks, DoD-1–16, and all plan phases.

Reviewed track-relevant changes versus `main`, including untracked `crates/pst-dedup-cli/src/attach_probe.rs`. The worktree also contains unrelated 0075/governance edits.

## Requirement and DoD Matrix

| Item | Status | Evidence / gap |
|---|---|---|
| DoD-1 Probe engine | Partial | L1–L3, chunked buffer, and budgets exist. Timeout is not a hard bound around blocking reads; materializer re-probe does not share total budgets. |
| DoD-2 Winner path | Partial | `probe_keep_set_groups` runs before resolve, but materializer performs a second unshared probe pass. |
| DoD-3 Scan path | Partial | Flag exists, but scan lacks configurable peer-cap CLI wiring and deep-probe progress reporting. |
| DoD-4 Reasons | Partial | Public strings match 0073; `PropertyNotFound` during stream open is misclassified as `ATTACH_META_FAILED`. |
| DoD-5 Preflight | Partial | Rate math and escalation exist, but post-probe scan/strict tallies are stale or inconsistent. |
| DoD-6 Keep-set fidelity | Met, with weak integration evidence | Probe failures degrade candidates and ranking prefers clean peers. |
| DoD-7 Stream honesty | Met | Deep failures set `stream_available=false`; L2 success is not labeled full verification. |
| DoD-8 Safety | Partial | Probe LRU and fixed buffer exist; materializer caches remain unbounded and scan progress is unwired. |
| DoD-9 `parents_only` | Met | Probe is skipped for parents-only/no-attachments paths. |
| DoD-10 Honesty/docs | Partial | Unique-pst documentation is honest; scan documentation and cancellation coverage honesty are incomplete. |
| DoD-11 Timeout | Partial | Deadline checks occur between reads only; production blocking reads can exceed the deadline. |
| DoD-12 Peer cap | Partial | Probe count is bounded, but cap tally is underreported when exactly N peers are attempted. |
| DoD-13 Cache | Partial | Level-aware and mtime-aware cache exists, but no scan→unique sharing; materializer is cacheless. |
| DoD-14 Tests | Partial | Unit simulations exist; no production-path fixture/integration coverage proves the locked cases. |
| DoD-15 Docs | Partial | Unique-pst flags are documented; scan flags, peer-cap configuration, and 0081 cross-link are missing. |
| DoD-16 Recorded | Unmet | `review.md` missing; registry remains `Ready`; no D-0074 entry observed; Ledgerful status/commit unavailable. |

## Findings

### [P1] Post-probe scan results and strict tallies are inconsistent

Confidence: High  
Requirement: DoD-3, DoD-5, DoD-6, strict-skip and scan-tally honesty  
Location: `crates/pst-dedup-cli/src/scan.rs:574-613, 731-844`; `crates/pst-dedup-cli/src/unique_pst_cmd.rs:1051-1104`

Problem: Scan rows, per-file statistics, and the dedup index are finalized before deep probing. Later probe failures mutate only candidates and selected summary fields.

Failure scenario: A strict deep-probe failure increments `summary.skipped`, but `recoverable_messages`, `total_messages`, unique/duplicate counts, file-level skipped counts, and already-streamed CSV rows still describe the pre-probe result. Unique-pst removes strict candidates but does not recompute the preflight recommendation from the new skips.

Correction: Run probe results through one reconciliation path before emitting rows/statistics, or rebuild all affected tallies and reports after probing. Recompute strict preflight as fail-closed.

Verification: Add end-to-end scan and unique-pst tests asserting per-file counts, recoverable totals, CSV integrity rows, and strict recommendation.

Deferrable: No

### [P1] Materialize re-probing bypasses hard deep-probe budgets

Confidence: High  
Requirement: DoD-1, DoD-2, DoD-8; §§3.4 and 3.9  
Location: `crates/pst-dedup-cli/src/unique_pst_cmd.rs:1186-1197`; `crates/pst-dedup-cli/src/pst_materializer.rs:262-277`

Problem: Unique-pst performs a second probe pass without `max_attaches` or shared `max_probe_bytes`. Each attachment passes `deep_per` as the global budget, so `--deep-attach-max-probe-bytes 0` can still cause materializer reads for every winner.

Failure scenario: Large exports incur unbounded aggregate re-probe I/O beyond the configured hard budget, while summary counters only describe the first pass.

Correction: Share one budgeted `AttachProbeEngine`/cache across phases, or remove the duplicate probe. Bound materializer/export handle caches as part of the same path.

Verification: Test zero budgets, exact budget exhaustion, and aggregate bytes across preflight plus materialization.

Deferrable: No

### [P1] Per-attach timeout does not bound blocking reads

Confidence: High  
Requirement: DoD-11; §3.4.1  
Location: `crates/pst-dedup-cli/src/attach_probe.rs:575-618`

Problem: The deadline is checked before `reader.read`, but synchronous reads cannot be interrupted. A blocking read may exceed the configured wall-clock limit indefinitely.

The timeout test at `attach_probe.rs:1373-1400` starts with an already-expired deadline and does not exercise the production reader path.

Correction: Use an interruptible/bounded I/O strategy or isolate reads behind a cancellable worker with a defined cleanup policy. Test a read that actually exceeds the configured deadline.

Verification: Production-path timeout test with a blocking/slow reader and assertion that subsequent attachments continue.

Deferrable: No

### [P1] Scan cancellation produces incomplete but apparently non-truncated coverage

Confidence: High  
Requirement: Cancel honesty; §§3.4, 3.8, 3.9  
Location: `crates/pst-dedup-cli/src/scan.rs:696-736, 810-828`; `crates/pst-dedup-cli/src/attach_probe.rs:345-350`

Problem: Scan passes `progress=None`, and when probing is cancelled it leaves `attach_probe_truncated=false` and has no serialized `cancelled`/incomplete field or coverage note. The probe summary knows cancellation occurred, but `ScanSummary` does not.

Failure scenario: A library/GUI scan cancelled during deep probing can return an enabled probe report with partial attempted/failed counts and no explicit incomplete-coverage marker.

Correction: Wire scan progress, propagate cancellation into the summary, and mark coverage incomplete/truncated without counting cancellation as attach failure.

Verification: End-to-end `run_scan` cancellation test asserting JSON coverage and unchanged integrity degradation.

Deferrable: No

### [P2] Scan CLI does not expose the peer-probe budget

Confidence: High  
Requirement: DoD-3, DoD-12, DoD-15  
Location: `crates/pst-dedup-cli/src/main.rs:1159-1211`

Problem: `ScanOptions` has `deep_attach_max_peer_probes_per_group`, but `scan` has no corresponding argument and hard-codes `3`.

Failure scenario: Operators cannot configure the locked peer cap on the scan path, despite the other deep-probe budgets being configurable.

Correction: Add and document `--deep-attach-max-peer-probes` for scan, with validation and propagation.

Verification: CLI parse/help test and options propagation test.

Deferrable: No

### [P2] Attachment stream-open errors can be reported as metadata failures

Confidence: High  
Requirement: DoD-4; §3.5  
Location: `crates/dedup-engine/src/integrity.rs:281-297`; `crates/pst-dedup-cli/src/attach_probe.rs:541-550`

Problem: `PropertyNotFound` and `PropertyTypeMismatch` are always mapped to `ATTACH_META_FAILED`, although `open_attachment_data` can return these for missing/unreadable payload data. The specification reserves `ATTACH_META_FAILED` for attachment-list failures.

Correction: Distinguish list/metadata context from stream-open context; map payload-open failures to `ATTACH_STREAM_OPEN_FAILED`.

Verification: Add stream-open missing-payload tests distinct from list-metadata failure tests.

Deferrable: No

### [P2] Locked acceptance cases are not proven through production paths

Confidence: High  
Requirement: DoD-14; §3.11  
Location: `crates/pst-dedup-cli/src/attach_probe.rs:955-1400`

Problem: The new tests mostly use tokens usedmissing paths, direct rank assertions, duplicated discard loops, or an already-expired deadline. There are no deep-probe integration tests using a real/synthetic PST fixture for CRC/open/read behavior, valid LRU eviction, scan summary reconciliation, or CLI wiring.

Correction: Add fixture-backed or production-compatible synthetic tests for each locked case, especially CRC/read failures, valid handles over capacity, full-level behavior, cancellation, and scan/unique end-to-end output.

Verification: Run the full locked §3.11 matrix and demonstrate old behavior fails the new tests.

Deferrable: No

### [P2] Peer-cap reporting misses the exact-N case

Confidence: High  
Requirement: DoD-12; §3.7.1  
Location: `crates/pst-dedup-cli/src/attach_probe.rs:828-946`

Problem: `peer_probe_capped_groups` increments only when the loop encounters an additional candidate after N probes. A group with exactly N failed/no-clean candidates is capped semantically but is not counted.

Correction: Record the cap whenever N attempts complete without a clean peer, whether or not another candidate remains.

Verification: Test groups with N−1, N, and N+1 dirty peers.

Deferrable: No

## Completeness Sweep

No new `TODO`, `FIXME`, `stub`, `fake`, `unimplemented!`, or placeholder matches were found in the touched implementation.

Known residual/no-op surfaces remain explicitly documented:

- GUI deep-attach checkbox is not wired.
- Materializer and export stream caches use unbounded `HashMap`s.
- Scan→unique cache sharing is absent.
- CRC fixture coverage is not present.
- `stream_available_hint` exists but has no production caller.

No source mutation or automatic ScanPST path was found. Probe buffers are bounded and no probe `read_to_end` path was introduced.

## Wiring and Regression Review

The main production path is reachable:

`scan/unique-pst CLI → run_scan → candidates → probe engine → fidelity mutation → resolve_groups → PstMaterializer → writer/0073 ledger`.

The major wiring defects are post-probe reporting reconciliation, the unbudgeted materializer re-probe, missing scan progress/cancellation coverage, and missing scan peer-cap configuration.

Reason serialization, threshold math, default-off behavior, parents-only skipping, and 0073 public strings are present. No migration or signing boundary changes were found.

## Verification Evidence

Observed:

- `cargo fmt --all --check` — passed.
- `cargo metadata --no-deps --format-version 1` — passed.
- `git diff --check main` — reports trailing whitespace on the modified track status line in `spec.md`.
- `ledgerful ledger status --compact` — unavailable: `unable to open database file`.
- `ledgerful scan --impact` — unavailable in read-only mode: `Failed to write report`.
- Existing `target\debug\pst-dedup.exe --help` was stale and did not contain the new flags; it was not treated as current-build evidence.
- No files or Git state were modified by this review.

Reported by orchestrator, not independently observed:

- `cargo test -p dedup-engine --lib` — 93 pass.
- `cargo test -p pst-dedup-cli --lib` — 67 pass.
- Touched-crate clippy — pass.
- GUI check — pass.

Not verifiable here:

- Full workspace clippy/test gates.
- Ledger transaction/signature verification.
- Canonical review artifact and registry completion.

## Deferred Candidates

Only after the P1/P2 findings are fixed, these qualify as difficult, non-blocking P3 residual candidates:

- `D-0074-gui` — GUI deep-attach checkbox and controls.
- `mat-lru` — bounded materializer/export stream handle caches.
- `cache-share` — shared in-process cache across scan and unique-pst.
- `crc-fixture` — real synthetic CRC integration fixture.

These residuals must not be used to defer the findings above.

## Completion Decision

FAIL. The track is not complete. Fix the P1/P2 findings, add production-path regression tests, rerun the full verification gates, reconcile Ledgerful provenance, record only qualifying P3 residuals, and then perform a fresh independent review.

245,622

