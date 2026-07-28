# Track Completion Audit — 0078-UniqueExportExitCodes

## Verdict: PASS WITH DEFERRED P3

## Scope Reviewed

Read-only review of the working tree on `track/0078-unique-export-exit-codes`, including spec, plan, prior reviews, implementation, tests, docs, registry, and deferred records.

No files or Git state were modified.

## Prior Finding Dispositions

| Finding | Disposition | Evidence |
|---|---|---|
| P1 keep-set summary contract | Fixed | `resolve_keep_set_summary_path` always anchors a summary, including stdout-only mode; write failure reclassifies `report_ok=false` (`keep_set_cmd.rs:65-95,280-372`). Test: `keep_set_stdout_only_still_self_locating`. |
| P2 attach-fidelity acceptance | Fixed | Real `write_canonical_eml` with `NullAttachStreamSource` produces `attachments_failed=1`, then maps through `classify_export` to fidelity `partial`, exit 64 (`export_exit_0078.rs:350-419`). |
| P2 quarantine collision | Fixed | Millisecond timestamp plus `_2`, `_3`, ... collision probing (`unique_pst_cmd.rs:812-890`). Existing collision test preserves both partials. |

## Requirement and DoD Matrix

| DoD | Status | Evidence / Gap |
|---:|---|---|
| 1 | Met | Pure classifier and unit matrix in `export_outcome.rs:155`. |
| 2 | Met | `run -> Result<CliExit>` and normal `ExitCode` mapping (`main.rs:907,929`). |
| 3 | Met | `compute_export_ok` retained and expressed through `classify_export` (`unique_pst_cmd.rs:2739`). |
| 4 | Met | Frozen codes 0–5 unchanged; new codes are 64, 65, 130. |
| 5 | Met | Attach soft-fail maps to partial/64; production writer acceptance proves the data path. |
| 6 | Partial | Risk gate wiring is present; process-level exit-65 E2E remains deferred. |
| 7 | Met | Cancellation writes summary and returns 130. |
| 7a | Met | Cancellation emits only `["CANCELLED"]`. |
| 8 | Met | Hard failures remain exit 1 with invalid/absent artifact state. |
| 9 | Met | Process-status/summary equality covered by clean unique-pst, unique-eml, and keep-set tests. |
| 10 | Met | Contract fields are emitted by all three commands; stdout-only keep-set is now self-locating. |
| 11 | Met | Allow-partial and mutually exclusive fidelity flags are wired. |
| 12 | Met | Unique-eml counters feed the shared classifier; forced writer soft-fail yields 64. |
| 13 | Met | Exit reasons are closed constants; no PST-derived strings. |
| 14 | Met | Refinement assertion covers prior non-zero outcome classes. |
| 15 | Met | Cancellation outranks risk and attachment findings. |
| 16 | Met | README and export documentation contain the matrix and PowerShell dispatch. |
| 17 | Met | Deferred records correctly narrow D-0073-eml and retain the other residuals. |
| 18 | Met | Conductor/sequencing rows and 0081 handoff are updated. |
| 19 | Partial | Quarantine implementation and unit tests pass; multi-volume mid-write/retry E2E is deferred. |
| 20 | Met | Risk plus attachment failure produces ordered cumulative reasons. |
| 21 | Met | Closed artifact-state vocabulary and rename-failure test exist. |
| 22 | Met | Absolute summary paths and human-mode stderr routing are implemented. |
| 23 | Met | Anti-retry guidance names `AuditChainBroken`. |
| 24 | Handoff | Canonical `review.md` is orchestrator-owned and not yet present; full workspace gate was not independently runnable in read-only mode. |

## Findings

### [P3] Process-level exit-65 E2E remains outstanding

Confidence: High  
Requirement: DoD-6 / Verification 6  
Location: `crates/pst-dedup-cli/tests/export_exit_0078.rs`  
Problem: Risk-gate behavior is unit-tested and wired, but no process test exercises a CRC-risk fixture and verifies actual status 65.  
Evidence: `classify_export` and CLI parser are present; focused integration suite has no exit-65 process case.  
Failure scenario: A CLI plumbing regression could disconnect `--fail-on-export-risk` while unit tests remain green.  
Correction: Add the corrupt-source process test.  
Verification: Run it against the real binary and compare status with `summary.json.exit_code`.  
Deferrable: Yes

### [P3] Multi-volume mid-write cancellation/retry E2E remains outstanding

Confidence: High  
Requirement: DoD-19 / Verification 7a  
Location: `crates/pst-dedup-cli/src/unique_pst_cmd.rs:791-890`  
Problem: Quarantine and collision behavior are unit-tested, but a real multi-volume cancellation followed by plain retry is not covered.  
Correction: Add the difficult process-level fixture test.  
Verification: Confirm every volume is quarantined, `--out` is free, and retry without `--overwrite` succeeds.  
Deferrable: Yes

### [P3] Full unique-eml attachment ledger CSV parity remains deferred

Confidence: High  
Requirement: Narrow D-0073-eml residual  
Location: `crates/pst-dedup-cli/src/unique_eml_cmd.rs:396-412`  
Problem: Unique-eml now has the required data-path counters, but not the full ledger CSV parity.  
Correction: Implement under the D-0073 residual.  
Verification: Validate locus, reason taxonomy, and row-cap behavior.  
Deferrable: Yes

### [P3] Cross-process cancellation remains deferred

Confidence: High  
Requirement: D-0045-02  
Location: `docs/deferred.md:408`  
Problem: 0078 covers in-process cancellation only.  
Correction: Implement and verify cross-process job cancellation separately.  
Deferrable: Yes

### [P3] Retryability classification remains deferred

Confidence: High  
Requirement: D-0078-retryable  
Location: `docs/deferred.md:820`  
Problem: JSON does not yet distinguish transient from permanent failures.  
Correction: Add the future `retryable` taxonomy without expanding exit-code meanings.  
Deferrable: Yes

### [P3] GUI fidelity/exit surfacing remains deferred

Confidence: High  
Requirement: D-0078-gui  
Location: `crates/pst-dedup-gui/src/unique_worker.rs:60-85`  
Problem: GUI carries fidelity and exit code internally but does not fully surface the classification in the UI.  
Correction: Add explicit fidelity/exit-reason presentation.  
Deferrable: Yes

## Completeness Sweep

No new scoped TODO, FIXME, stub, fake-success path, `process::exit`, or PST-derived `exit_reason` was found. The deterministic `NullAttachStreamSource` is an intentional soft-failure test fixture, not production behavior.

## Wiring and Regression Review

Verified end-to-end paths:

- CLI entry → `run` → unique command → shared classifier → summary JSON → actual process exit.
- Keep-set stdout-only mode → summary next to first input → absolute self-locating path.
- Unique-eml writer attachment counter → shared classifier → fidelity/exit contract.
- Cancellation → volume quarantine → cancellation summary → exit 130.
- Frozen legacy codes remain unchanged.

## Verification Evidence

Observed now:

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- Fixture exists: `fixtures/aspose_outlook.pst`.
- Compiled targeted test binary lists all 8 expected tests.
- Static placeholder and `process::exit` sweeps found no new scoped issues.

Reported by orchestrator/user:

- `cargo test -p pst-dedup-cli --test export_exit_0078`: 8 passed.
- `cargo clippy -p pst-dedup-cli --all-targets -- -D warnings`: passed.

Not independently runnable here:

- Cargo test/clippy execution attempted but was blocked before compilation by read-only access to `C:\dev\Dedupe\target\debug\.cargo-build-lock`.
- Full workspace cargo gate was not independently observed.
- Ledgerful status/impact commands were unavailable because the database/report requires write access; cached impact data is stale against the current dirty tree.

## Completion Decision

The prior P1 and P2 findings are fixed. No P0–P2 findings remain. Engineering review passes with only the six explicitly allowlisted difficult P3 residuals above.

The orchestrator should run the full cargo gate, write canonical `conductor\0078-UniqueExportExitCodes\review.md`, and complete the Ledgerful handoff before marking the track Completed.