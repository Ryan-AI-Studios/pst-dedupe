# Track Completion Audit — 0074-DeepAttachPreflightFidelity

## Verdict: FAIL

Blocking P1/P2 gaps remain. The requested fixes are substantially wired, but strict-output reconciliation, Full-level timeout budgeting, cache identity, and production-path test coverage are incomplete.

## Scope Reviewed

Reviewed the full `spec.md`, `plan.md`, both prior FAIL reviews, current uncommitted implementation, tests, CLI wiring, materializer path, docs, worktree, and governance state.

The branch points at the same commit as `main`; implementation is uncommitted. Unrelated 0075 edits were excluded except where shared files overlap.

## Requirement and DoD Matrix

| DoD | Status | Evidence |
|---|---|---|
| DoD-1 Probe engine | Partial | L1–L3 chunked probing exists, but Full timeout accounting undercharges the global budget. |
| DoD-2 Winner path | Met | Unique-pst probes before resolve and passes `ProbeResultCache` into materialization. |
| DoD-3 Scan path | Met | Scan flag, budgets, peer cap, and stderr progress are wired. |
| DoD-4 Reasons | Met | Additive codes and 0073-compatible strings; stream-open mapping fixed. |
| DoD-5 Preflight | Partial | Aggregate counts recalculate, but strict row/file outputs can remain inconsistent. |
| DoD-6 Keep-set fidelity | Met, weakly proven | Probe failures degrade candidates and clean peers rank first. |
| DoD-7 Flags | Met | Cached failures set `stream_available=false` without re-I/O. |
| DoD-8 Safety | Partial | Fixed discard buffer, cancellation, progress, and probe LRU exist; timeout accounting remains unsafe for Full. |
| DoD-9 `parents_only` | Met | Deep probing is skipped. |
| DoD-10 Honesty | Met | Coverage fields and re-export/ScanPST guidance are documented. |
| DoD-11 Timeout | Partial | Worker timeout returns promptly, but Full workers can consume more than charged. |
| DoD-12 Peer cap | Met | Configurable cap and exact-N tally are implemented. |
| DoD-13 Cache | Partial | Level-aware and mtime-aware, but no source PST file-size fingerprint; scan-to-unique sharing remains deferred. |
| DoD-14 Tests | Partial | Unit tests were added, but locked acceptance cases are not proven through production paths. |
| DoD-15 Docs | Met | Scan and unique-pst flags, defaults, budgets, 0073 interaction, and residual guidance are documented. |
| DoD-16 Recorded | Pending closeout | Registry remains `Ready`; no canonical `review.md`, D-0074 ledger entries, or ledger commit observed. |

## Findings

### [P1] Strict scan reconciliation leaves stale duplicate relationships

Confidence: High  
Requirement: DoD-3, DoD-5, DoD-6  
Location: [scan.rs:828](C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:828), [scan.rs:931](C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:931), [report.rs:94](C:/dev/Dedupe/crates/dedup-engine/src/report.rs:94)

Problem: Post-probe code recomputes aggregate counts but only updates `row.integrity`. It does not rebuild each buffered row’s `DedupResult`.

Failure scenario: If a strict probe skips the original winner, a surviving duplicate row can still report `DuplicateOf` that omitted message in the dedup CSV and retained rows.

Correction: Rebuild row results from the post-probe candidate set before writing buffered rows, or explicitly promote affected duplicates to unique/recompute relationships.

Verification: Strict fixture test where the original winner is probe-skipped and its duplicate survives; assert CSV contains no reference to the skipped message.

Deferrable: No

### [P1] Unique-pst strict probe skips leave per-file scan statistics stale

Confidence: High  
Requirement: DoD-2, DoD-5, DoD-6  
Location: [unique_pst_cmd.rs:1116](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:1116), [unique_pst_cmd.rs:1147](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:1147), [unique_pst_cmd.rs:1176](C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:1176)

Problem: Strict probe skips update aggregate `skipped` and `recoverable_messages`, but do not update `summary.files[*]`, `partial_files`, `opened_files`, or per-file recoverable/skipped counts.

Failure scenario: `summary.json` reports fewer recoverable messages overall while the per-file section still claims the skipped message was recoverable and the file remained opened.

Correction: Apply the same post-probe per-file reconciliation used by scan, including status and reason tallies.

Verification: Strict unique-pst fixture with a failed attachment; assert aggregate and per-file summaries agree.

Deferrable: No

### [P1] Full-level timeout can exceed the configured global byte budget

Confidence: High  
Requirement: DoD-1, DoD-8, DoD-11  
Location: [attach_probe.rs:571](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:571), [attach_probe.rs:680](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:680)

Problem: Full-level workers may read up to `global_bytes_left`, but timeout accounting reserves only `per_attach_max_bytes`.

Failure scenario: With Full probing, a timed-out worker can continue reading nearly the entire remaining global budget while the engine charges only the 1 MiB L2 cap, then permits another probe. Reported bytes and actual I/O diverge.

Correction: Reserve the effective per-probe budget (`global_bytes_left` for Full) before dispatch, or cancel/join the worker before permitting another probe.

Verification: Full-level slow-reader test with multiple timeouts; assert actual reads and reported bytes never exceed `max_probe_bytes`.

Deferrable: No

### [P2] Cache identity does not include source PST file size

Confidence: Medium  
Requirement: DoD-13, §3.10  
Location: [attach_probe.rs:117](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:117), [attach_probe.rs:340](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:340), [attach_probe.rs:446](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:446)

Problem: The cache key’s `size` is attachment metadata size, not source PST file size. Source identity is otherwise only path plus mtime rounded to seconds.

Failure scenario: A PST is replaced at the same path with the same-second mtime and matching attachment metadata size; stale probe results can be reused.

Correction: Include source PST file size plus sufficiently precise mtime, or another file fingerprint, in the cache key.

Verification: Cache test replacing a file at the same path and asserting the old result misses.

Deferrable: No

### [P2] Added tests still do not prove the locked production paths

Confidence: High  
Requirement: DoD-14, §3.11  
Location: [attach_probe.rs:1748](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:1748), [attach_probe.rs:1788](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:1788), [attach_probe.rs:1343](C:/dev/Dedupe/crates/pst-dedup-cli/src/attach_probe.rs:1343)

Problem: Timeout tests use standalone simulated readers/channels rather than `probe_attach_stream_timed`; LRU tests cover missing paths rather than valid eviction/live-handle limits. No production fixture test proves strict CSV reconciliation, materializer cache propagation, or deep-probe scan output.

Correction: Add production-compatible fixture tests for timeout invocation, valid LRU eviction, cache-backed `stream_available`, strict scan CSV/integrity output, and unique-pst summary reconciliation.

Verification: Execute the complete §3.11 matrix against the actual scan/materializer/unique-pst paths.

Deferrable: No

## Completeness Sweep

No new TODO/FIXME/stub/unimplemented/placeholder matches were found in touched implementation files.

Positive findings:

- No source mutation or automatic ScanPST execution.
- Probe discard buffer is fixed at 64 KiB.
- No per-page CRC logging was introduced.
- Deep probing remains opt-in.
- Parents-only paths skip probing.
- Prior reason-mapping and exact-N peer-cap findings are fixed.
- P1-A cache wiring and P1-D cancel-before-probe reporting are fixed.
- P2-A scan progress is wired.

## Verification Evidence

Observed:

- `cargo fmt --all --check` — passed.
- `git diff --check` — passed.
- `cargo metadata --no-deps --format-version 1` — passed.
- Placeholder sweep — no matches.
- Cargo tests were blocked by read-only fitokens usedlesystem access to `target\debug\.cargo-build-lock`.
- Ledgerful status failed with `unable to open database file`.
- Ledgerful impact scan could not write `.ledgerful/reports/latest-scan.json`.
- No files or Git state were modified.

Reported by orchestrator, not independently observed:

- dedup-engine lib: 94 passed.
- pst-dedup-cli lib: 76 passed.
- Touched-crate clippy: passed.
- GUI check: passed.

## Deferred Candidates

These are valid difficult P3 candidates only after the blocking findings are fixed:

- `D-0074-gui`
- `D-0074-mat-lru`
- `D-0074-cache-share`
- `D-0074-crc-fixture`
- `D-0074-timeout-join` — only the worker-joining residual, not the current Full-budget accounting defect.

## Completion Decision

FAIL. Fix the five findings, add production-path regression coverage, rerun the full gates, reconcile Ledgerful provenance, record only approved P3 deferrals, and perform another independent final review.

240,508

