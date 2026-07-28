# Track Completion Audit — 0078-UniqueExportExitCodes

## Verdict: FAIL

## Scope Reviewed

Reviewed the complete `spec.md`, `plan.md`, baseline, implementation notes, internal reviews, uncommitted working tree, affected CLI/GUI code, tests, documentation, registry, and deferred records.

Ledgerful and AI-Brains were unavailable because both reported `unable to open database file`. Cached impact data marked the tree high-risk.

## Requirement and DoD Matrix

| DoD | Status | Evidence / Gap |
|---|---|---|
| 1 | Met | Pure classifier and unit matrix present. |
| 2 | Met | `run -> Result<CliExit>` and normal `main` mapping. |
| 3 | Met | `compute_export_ok` retained and routed through classifier. |
| 4 | Met | Codes 0–5 and mapping tests preserved. |
| 5 | Met | Attach soft failures remain data-path partial fidelity; artifact retained. |
| 6 | Met | Risk gate defaults off and is opt-in. |
| 7 | Met | Cancellation returns 130 and writes cancellation summary. |
| 7a | Met | Cancellation reason is only `CANCELLED`. |
| 8 | Met | Hard failures classify as exit 1 and use invalid artifact state when needed. |
| 9 | Partial | Real-process equality is tested for selected 0/64 paths; no equivalent coverage for risk 65 or all export variants. |
| 10 | Partial | unique-pst and unique-eml fields exist; keep-set lacks `summary_path`. |
| 11 | Met | Allow-partial and mutually exclusive flags are wired. |
| 12 | Partial | Production unique-eml counters exist, but no forced production-path attach-skip test. |
| 13 | Met | Closed reason vocabulary is used. |
| 14 | Met | Refinement table test exists. |
| 15 | Met | Cancellation precedence is tested. |
| 16 | Met | README/docs matrix and PowerShell dispatch exist. |
| 17 | Met | Deferred records are narrowed/annotated correctly. |
| 18 | Met | Registry rows and 0081 handoff references updated. |
| 19 | Partial | Quarantine helper is tested; required mid-write multi-volume retry E2E is absent. |
| 20 | Met | Risk plus attach yields 65 with cumulative reasons. |
| 21 | Met | Closed artifact states and rename-failure unit test exist. |
| 22 | Partial | Keep-set has no self-locating summary path; unique-eml path points to `manifest.json`, which does not contain the emitted summary fields. |
| 23 | Met | Anti-retry guidance names `AuditChainBroken`. |
| 24 | Pending | `review.md` and full workspace gate evidence remain orchestrator closeout work. |

## Findings

### [P1] Keep-set and unique-eml do not satisfy the self-locating summary contract

Confidence: High

Requirement: DoD-10, DoD-22, spec §3.5 and §3.7.

Location: `crates/pst-dedup-cli/src/keep_set_cmd.rs:91-100,243-280`; `crates/pst-dedup-cli/src/unique_eml_cmd.rs:420-448`

Problem: Keep-set JSON inserts `fidelity`, `exit_code`, `exit_reason`, and `artifact_state`, but never emits `summary_path`. Its human failure path also has no summary location. Unique-eml emits `summary_path` pointing at `manifest.json`, but that manifest does not contain the fidelity/exit summary fields.

Failure scenario: An automation consumer receives keep-set or unique-eml output and cannot reliably locate the JSON object containing the exit contract.

Correction: Define and emit a canonical summary path for keep-set and unique-eml, make it absolute and self-consistent, and add schema tests for all three commands.

Verification: Run JSON and human-mode tests for success, partial, hard-fail, and risk outcomes; assert the path exists and resolves to the summary containing the reported fields.

Deferrable: No

### [P2] Acceptance tests can silently skip the required attach-fidelity behavior

Confidence: High

Requirement: DoD-5, DoD-9, DoD-12; plan Phase 7.

Location: `crates/pst-dedup-cli/tests/export_exit_0078.rs:60-145`

Problem: `attach_soft_fail_exit_64_or_skip` does not force an attachment failure. It returns successfully when the fixture has no failures and silently skips when stdout is not JSON. There is also no production-path unique-eml test proving a forced attach skip reaches exit 64.

Failure scenario: A regression that disconnects unique-eml counters, emits malformed JSON, or routes partial attachment failures through exit 1 can pass without failing the test suite.

Correction: Use a deterministic forced-failure fixture or injectable attachment source, fail closed on malformed output, and add unique-eml and keep-set contract assertions.

Verification: Assert actual process status, fidelity, reason, artifact state, and `summary_path` for forced partial and allowed-partial cases.

Deferrable: No

### [P2] Quarantine names can collide within the same second

Confidence: High

Requirement: Spec §3.6, DoD-19, plan quarantine safety lock.

Location: `crates/pst-dedup-cli/src/unique_pst_cmd.rs:814-827,860-868`

Problem: Quarantine destinations use only Unix seconds. Two cancellations of the same output in one second produce the same destination; on Windows the second rename can fail.

Failure scenario: The second cancellation leaves the partial PST at `--out`, reports `invalid_in_place`, and prevents a plain retry despite an otherwise healthy quarantine path.

Correction: Use a collision-resistant timestamp/unique suffix or probe for an unused destination without overwriting existing quarantined artifacts.

Verification: Add a test invoking quarantine twice with the same timestamp and assert both artifacts are retained and `--out` is free.

Deferrable: No

## Completeness Sweep

No new production TODOs, stubs, `process::exit`, fake success paths, or PST-derived reason strings were found in the track implementation.

The explicit test skip paths above are incomplete verification. The pre-existing `--also-eml` residual remains documented and is outside this track.

## Wiring and Regression Review

The unique-pst path is wired end to end:

`main -> run -> unique-pst -> export data -> risk/classification -> summary -> CliExit`.

Unique-eml uses the same classifier and data-path attachment counters. Frozen codes 0–5 remain unchanged, and partial attachment failures do not use `export.error` to force generic exit 1.

Cancellation quarantine covers completed volume siblings and does not delete artifacts. However, collision handling is not deterministic-safe.

GUI argument construction remains compatible; fidelity display remains explicitly deferred as D-0078-gui.

Keep-set classification is reachable, but its JSON contract is incomplete.

## Verification Evidence

Observed:

- Working tree contains the stated uncommitted implementation.
- `fixtures/aspose_outlook.pst` exists.
- Internal reviews r1/r2 were read.
- `git diff --check` reports trailing whitespace in `docs/unique-pst-export.md`.
- Ledgerful status/impact and AI-Brains orientation failed due unavailable database files.

Reported by the orchestrator/user, not independently rerun:

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test -p pst-dedup-cli`: passed.

Not verifiable from the supplied gates:

- Full `cargo test --workspace`.
- Process-level risk exit 65.
- Production unique-eml forced attach partial.
- Multi-volume mid-write cancellation followed by plain retry.
- Keep-set/unique-eml self-locating summary contract.

## Deferred Candidates

These are difficult, non-blocking P3 candidates only:

- Process-level exit-65 fixture coverage.
- Full multi-volume mid-write cancellation/retry E2E.

Existing deferred items remain correctly open: D-0073-eml, D-0045-02, D-0078-retryable, and D-0078-gui.

## Completion Decision

FAIL. The core classifier and unique-pst exit plumbing are substantially implemented, but the keep-set/unique-eml summary contract is incomplete, required attach-path tests can silently skip, and quarantine naming has a real collision edge case. These issues must be fixed before completion.