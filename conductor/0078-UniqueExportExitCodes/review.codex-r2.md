# Track Completion Audit - 0078-UniqueExportExitCodes

## Verdict: FAIL

## Scope Reviewed

Working tree on `track/0078-unique-export-exit-codes` versus `origin/main`, including uncommitted and untracked changes, complete `spec.md`, `plan.md`, baseline, prior review, implementation notes, CLI/GUI wiring, tests, docs, registry, and deferred records.

Ledgerful status and impact commands were unavailable:

- `ledgerful ledger status --compact`: `unable to open database file`
- `ledgerful scan --impact`: failed to write `.ledgerful/reports/latest-scan.json`

Cached impact data reports high risk and a dirty tree.

## Prior Finding Dispositions

| Prior finding | Disposition | Evidence |
|---|---|---|
| P1 keep-set/unique-eml summary contract | Partly fixed; still open | Unique-eml now writes `{out}/summary.json`; keep-set writes `keep_set_summary.json` only when an output-path option is supplied. Stdout-only keep-set still emits `summary_path: ""`. Summary-write failures are logged and do not fail closed. |
| P2 acceptance tests silently skip attach fidelity | Partly fixed; still open | JSON parsing now fails closed and unique-eml/keep-set contract tests exist. However, attach tests still accept a clean fixture without proving a production attach failure, and unique-eml has no forced production-path attach-failure test. |
| P2 same-second quarantine collision | Validated fixed | Millisecond stamps, `_2`/`_3` probing, and same-stamp collision unit coverage are present at `unique_pst_cmd.rs:811-897,3675-3721`. |

## Requirement and DoD Matrix

| DoD | Status | Evidence / Gap |
|---|---|---|
| 1 | Met | Pure `ExportOutcome` classifier and unit matrix in `export_outcome.rs`. |
| 2 | Met | `run -> Result<CliExit>` and normal `main` mapping; no added `process::exit`. |
| 3 | Met | `compute_export_ok` retained and routed through `classify_export`; existing tests remain. |
| 4 | Met | Codes 0–5 and mappings unchanged; additive 64/65/130 only. |
| 5 | Partial | Unique-pst attach counters and exit 64 wiring exist, but acceptance coverage does not force attach failure. |
| 6 | Partial | Opt-in gate is wired and unit-tested; process exit-65 E2E remains absent. |
| 7 | Met | Cancellation produces 130 and writes cancellation summaries before returning. |
| 7a | Met | Cancellation reason is exclusively `["CANCELLED"]`. |
| 8 | Met | Hard failures classify as exit 1; unique-pst uses `invalid_in_place` for retained unsafe bytes. |
| 9 | Partial | Real-process equality is tested for selected clean/unique-eml/keep-set paths; no forced partial or risk-65 process coverage. |
| 10 | Partial | Fields exist in emitted summaries, but keep-set stdout-only has no absolute summary path. |
| 11 | Met | Allow-partial and mutually exclusive fidelity flags are wired. |
| 12 | Partial | Unique-eml data-path counters exist; no forced production attach skip test proves partial fidelity. |
| 13 | Met | Closed reason vocabulary; no PST-derived reason strings. |
| 14 | Met | Refinement table test is present. |
| 15 | Met | Cancellation precedence unit test exists. |
| 16 | Met | README/docs matrix and PowerShell dispatch are present. |
| 17 | Met | Deferred records are narrowed and annotated correctly. |
| 18 | Met | Registry rows and 0081 anti-retry handoff references are present. |
| 19 | Partial | Quarantine helper and unit coverage exist; full multi-volume mid-write/retry E2E is absent. |
| 20 | Met | Risk plus attach produces 65 with cumulative reasons in unit coverage. |
| 21 | Met | Closed artifact states and rename-failure test exist. |
| 22 | Partial | Unique-pst and normal unique-eml paths are self-locating; keep-set stdout-only and summary-write failures violate the guarantee. |
| 23 | Met | `AuditChainBroken` anti-blanket-retry guidance is documented. |
| 24 | Not verifiable / incomplete | `review.md` and full gate evidence were not independently observed; only the user-reported gates are available. |

## Findings

### [P1] Keep-set summary contract remains conditional and fail-open

Confidence: High

Requirement: DoD-10, DoD-22; spec §3.5 and §3.7.

Location: `crates/pst-dedup-cli/src/keep_set_cmd.rs:60-72,292-340`; `crates/pst-dedup-cli/src/unique_eml_cmd.rs:421-492`

Problem: Keep-set only creates `keep_set_summary.json` when `--keep-set-json`, `--decision-csv`, or `--integrity-csv` is supplied. The documented stdout-only invocation emits an empty `summary_path`, so it is not self-locating. Additionally, keep-set and unique-eml treat summary-file write failures as warnings while continuing with the classified result.

Failure scenario: A successful or failed keep-set run using only `--json` has no on-disk summary and no absolute summary location. If summary writing fails, automation may receive exit 0 or a non-zero contract pointing to a file that does not exist.

Correction: Always establish and write a canonical keep-set summary path, or explicitly make the output path mandatory. Treat summary serialization/write failure as a hard report failure and ensure the emitted/returned exit code and persisted JSON agree.

Verification: Add stdout-only keep-set success/failure tests, deterministic summary-write-failure tests, and assert absolute path existence plus process-status equality.

Deferrable: No

### [P2] Attach-fidelity acceptance remains unproven on production paths

Confidence: High

Requirement: DoD-5, DoD-9, DoD-12; plan Phase 7.

Location: `crates/pst-dedup-cli/tests/export_exit_0078.rs:78-167,194-256`

Problem: The attach test accepts a fixture with zero attach failures and exits successfully. The unique-eml process test explicitly uses `--no-attachments`. The only unique-eml attach proof is a pure classifier unit test, not the production writer/counter path.

Failure scenario: A regression disconnecting `attachments_failed` from unique-eml classification, or routing a real attach soft-failure through exit 1, can pass all acceptance tests.

Correction: Use a deterministic fixture or production-path failure injection that guarantees an attachment failure. Assert unique-pst and unique-eml `fidelity`, `ATTACH_SOFT_FAIL`, `artifact_state`, exit 64, persisted summary, and allow-partial behavior. Fixture absence must fail rather than return.

Verification: Run the forced-failure process tests and confirm actual process status equals `summary.json.exit_code`.

Deferrable: No

## Completeness Sweep

No new production `TODO`, `FIXME`, `unimplemented!`, `process::exit`, fake success path, or PST-derived reason string was found in the scoped implementation.

The remaining `--also-eml` no-op is an existing documented residual (`D-0071-also-eml`), outside this track. The export acceptance tests still contain fixture-based early returns.

Trailing whitespace is fixed: observed `git diff --check origin/main` passed.

## Wiring and Regression Review

Unique-pst is wired end to end:

`main -> run -> unique-pst -> export counters/risk -> classify_export -> summary.json -> CliExit`.

Unique-eml correctly feeds writer attachment failures into `classify_export`, writes a dedicated summary, and uses the shared artifact-state helper.

Keep-set correctly emits the expanded contract when a disk artifact anchor exists, but its stdout-only path is disconnected from a durable self-locating summary.

Cancellation quarantine is collision-resistant and preserves completed volumes. The PST writer’s same-directory temporary file is removed on mid-write cancellation, so the prior same-second naming defect is fixed without leaving an unsafe final PST at `--out`.

## Verification Evidence

Observed:

- `cargo fmt --all --check`: passed.
- `git diff --check origin/main`: passed.
- Fixture exists at `fixtures/aspose_outlook.pst`.
- Working tree contains the stated fixes and untracked `export_outcome.rs` / `export_exit_0078.rs`.
- Prior review and internal re-review artifacts were read.

Reported by the orchestrator/user, not independently rerun:

- targeted clippy
- `export_outcome` tests
- `export_exit_0078` tests
- `unique_pst` tests
- `unique_eml` tests

Full workspace clippy/test gates were not independently observed because this review was read-only.

## Deferred Candidates

These qualify as difficult, non-blocking P3 candidates only:

- Process-level exit-65 E2E.
- Multi-volume mid-write cancellation plus plain-retry E2E.
- D-0073-eml full ledger CSV parity.
- D-0045-02 cross-process cancellation.
- D-0078-retryable.
- D-0078-gui.

The P1/P2 findings above are not deferrable.

## Completion Decision

FAIL. The quarantine collision fix is validated, but the keep-set/summary contract remains incomplete and attach-fidelity acceptance still does not exercise forced production failures.