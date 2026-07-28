# Track Completion Audit — 0078

## Verdict: PASS WITH DEFERRED P3

Core contract is implemented and lock-compliant: `run → Result<CliExit>`, pure `classify_export` with cumulative reasons (no short-circuit of the reason set), codes **64 / 65 / 130**, cancel quarantine of all volumes, JSON fidelity fields, unique-eml data-path attach counters, frozen 0–5, refinement-only, and attach soft-fail does **not** abuse `export.error` into hard-fail exit 1. Remaining gaps are proof-test polish, naming nits, and process DoDs expected open at first subagent pass.

## Scope Reviewed

| Artifact | Path |
|---|---|
| Spec / plan / baseline / notes | `conductor/0078-UniqueExportExitCodes/{spec,plan,baseline,implementation-notes}.md` |
| Outcome classifier | `crates/pst-dedup-cli/src/export_outcome.rs` |
| Exit enum / mapping | `crates/pst-dedup-cli/src/error.rs` |
| Plumbing | `crates/pst-dedup-cli/src/main.rs` (`run → Result<CliExit>`, `Ok(code) => code.into()`) |
| unique-pst + quarantine | `crates/pst-dedup-cli/src/unique_pst_cmd.rs` |
| unique-eml counters + classify | `crates/pst-dedup-cli/src/unique_eml_cmd.rs` |
| keep-set JSON fields | `crates/pst-dedup-cli/src/keep_set_cmd.rs` |
| Summary schema | `crates/pst-dedup-cli/src/unique_export_report.rs` |
| Integration tests | `crates/pst-dedup-cli/tests/export_exit_0078.rs`, `unique_pst.rs` (attach fail non-zero) |
| GUI compile surface | `crates/pst-dedup-gui/src/{unique_wizard,unique_worker}.rs` |
| Docs | `docs/unique-pst-export.md`, `README.md`, `docs/deferred.md` |
| Registry | `conductor/conductor.md`, `conductor/sequencing.md` |
| SIGINT | `crates/pst-dedup-cli/src/runner_util.rs` (no `process::exit`) |

Branch (handoff): `track/0078-unique-export-exit-codes`. Working tree vs origin/main. No `review.md` yet (expected).

---

## Requirement and DoD Matrix

| DoD | Status | Evidence |
|---|---|---|
| **1** `ExportFidelity` / `ExportOutcome` / `classify_export` pure + unit-tested | **Met** | `export_outcome.rs` full matrix: complete, partial 64, allow→0, hard 1, risk 65, cancel 130, cumulative, refinement, hard-outranks |
| **2** `run → Result<CliExit>`; main maps; no `process::exit` | **Met** | `main.rs:907-914`, `929`; unique-pst/eml return `Result<CliExit>`; only comments mention `process::exit` in `runner_util.rs` |
| **3** `compute_export_ok` retained via classify; existing tests unmodified | **Met** | `unique_pst_cmd.rs:2707-2718` re-expression; tests `3096-3118` still assert `compute_export_ok` only (no body edits to assertions) |
| **4** Codes 0–5 frozen (value, mapping, tests) | **Met** | `error.rs:12-24` Success…MatterIo; mapping tests `214-238` unchanged in role; new variants 64/65/130 only |
| **5** Exit 64 on message-complete + attach soft-fail; artifact retained | **Met** | Attach does **not** set `export_error` (`2100-2107`); `classify` → `PartialFidelity`; integration `export_exit_0078.rs` / `unique_pst_attachment_failures_force_export_fail` |
| **6** Exit 65 only with `--fail-on-export-risk` and rank met | **Met (unit)** | Default `RiskGate::Off`; units `risk_gate_not_export_ready_65`, `risk_gate_off_no_65`; wiring `unique_pst_cmd.rs:2287-2345`. **No process-level fixture E2E** (P3) |
| **7** Exit 130 on cancel; cancelled summary first | **Met** | Early + late cancel write summary then `CliExit::Cancelled`; `export_exit_0078::cancel_exit_130_and_summary`; unit cancel on scan stage |
| **7a** Cancelled reasons `["CANCELLED"]` only | **Met** | Early return in `classify_export` when cancelled; cancelled summary hardcodes same; cancel test asserts |
| **8** Hard fail still exit 1 | **Met** | `hard_fail → CliExit::Generic`; count/verify/export_partial/report dimensions |
| **9** `summary.exit_code` == process status (integration) | **Met (partial classes)** | `export_exit_0078`: clean 0, attach 64 path, cancel 130. Exit 65 not process-asserted (P3) |
| **10** fidelity/exit_code/exit_reason on unique-pst, unique-eml, keep-set; `ok` ↔ complete | **Met** | `UniqueExportSummary` fields; unique-eml `UniqueEmlSummaryOut`; keep-set JSON inserts; `ok = fidelity == Complete` (unique-pst also `&& !cancelled`) |
| **11** `--allow-partial-fidelity` → 0 + partial; mutual exclusion → 2 | **Met** | `into_cli_args` + unique-eml main; `mutual_exclusion_fidelity_flags_exit_2`; allow path in `export_exit_0078` |
| **12** unique-eml data-path counters; forced skip → partial | **Met (code); weak test** | `attach_parts_failed` on manifest + `ExportOkInput.attach_failed_total`; D-0073-eml comment. **No dedicated unique-eml → 64 test** (P3) |
| **13** Closed `exit_reason` vocab; no PST-derived strings | **Met** | `reason::*` constants only; reasons never interpolate paths/subjects |
| **14** Refinement assertion table | **Met** | `refinement_assertion_non_zero_stays_non_zero` + `baseline.md` |
| **15** Precedence: cancel + attach + risk → 130 | **Met** | `cancelled_outranks_all_only_cancelled_reason` |
| **16** Matrix in README + `docs/unique-pst-export.md` + PS dispatch | **Met** | README exit table; `unique-pst-export.md` §0078 with severity-first + PowerShell `switch` |
| **17** deferred.md updates | **Met** | D-0073-eml narrowed; D-0045-02 annotated; D-0078-retryable + D-0078-gui added |
| **18** conductor / sequencing rows; cross-link 0081 | **Met (In Progress)** | Rows present; status **In Progress** (completion flip is orchestrator); 0081 handoff in notes + docs |
| **19** Cancel mid-write quarantines every volume; `--out` free; plain retry | **Met (unit)** | `quarantine_cancelled_volumes` loops `volume_path_for`; multi-volume rename test; `!out.exists()`. Full mid-write multi-volume **process** retry E2E not present (P3) |
| **20** Cumulative reasons risk+attach → 65, `["RISK_GATE","ATTACH_SOFT_FAIL"]` | **Met** | `cumulative_risk_and_attach` unit |
| **21** `artifact_state` closed vocab; rename-fail → `invalid_in_place` | **Met** | `artifact_state_for` + `quarantine_rename_failure_is_failed` |
| **22** `summary_path` absolute; human `summary: ` on stderr non-zero | **Met** | unique-pst `std::path::absolute`; `run_unique_pst` stderr line; unique-eml human stderr absolute; unique-eml JSON uses resolved absolute-ish path via `resolve_cli_path_maybe_missing` |
| **23** 0081 handoff anti-recommendation + `AuditChainBroken` | **Met** | `implementation-notes.md`, README, `unique-pst-export.md` cross-link; deferred D-0078-retryable |
| **24** `review.md` + full cargo gate | **Unmet (process)** | This file is subagent audit, not final `review.md`; full workspace gate not re-run by auditor (implementer claims fmt/clippy + `cargo test -p pst-dedup-cli`) |

### Locked rules check (spec §2.3 / plan Locks)

| Lock | Status |
|---|---|
| 0–5 frozen | **Met** — new codes only 64/65/130 |
| New codes 64–113 (+ conventional 130) | **Met** |
| Refinement only | **Met** — DoD-14 + baseline narrative |
| JSON truth; integer routing | **Met** |
| `ok == (fidelity == complete)` | **Met** (cancel forces `ok=false` with failed fidelity) |
| One vocabulary complete\|partial\|failed | **Met** |
| Cancellation not failure; outranks | **Met** — 130 + reasons only CANCELLED |
| Codes from data not logs | **Met** |
| Attach failure non-zero by default | **Met** — fail-on-partial default on |
| No `process::exit` / self-signal for 130 | **Met** |
| Closed `exit_reason` | **Met** |
| D-0073-eml / D-0045-02 not closed | **Met** |
| Quarantine never delete; don’t leave truncated at `--out` silently | **Met** (rename; `invalid_in_place` if fail) |
| Precedence picks integer only; reasons cumulative | **Met** — hard→risk→soft collected fully |
| FOCUS: no short-circuit reasons | **Met** |
| FOCUS: quarantine all volumes | **Met** — `1..=volume_count.max(1)` |
| FOCUS: `compute_export_ok` tests unmodified | **Met** |
| FOCUS: partial must not hard-fail as exit 1 via `export.error` | **Met** — explicit comment + no `export_error` on attach soft-fail (`2100-2107`) |

---

## Findings (P0–P3 format)

### [P3] unique-eml DoD-12 lacks a dedicated automated proof that attach soft-fail → exit 64
**Confidence:** High  

**Where:** `crates/pst-dedup-cli/src/unique_eml_cmd.rs` (wiring present); no `tests/unique_eml.rs` / `export_exit_0078` case  

**Evidence:** Counters and `classify_export` are wired (`attach_parts_failed` → `attach_failed_total`). Verification plan #8 / DoD-12 text still expects a forced-skip → partial/64 assertion.  

**Impact:** Low behavioral risk (path is mechanical); residual proof gap only.  

**Disposition:** Defer as polish / add a small unit or integration test before final close if desired.

---

### [P3] Exit 65 has strong unit coverage but no process-level fixture assertion
**Confidence:** High  

**Where:** `export_outcome.rs` units; `unique_pst_cmd` risk_gate wiring; missing `export_exit_0078` / `crc_integrity_0077` process assert  

**Evidence:** DoD-6 default-off and rank gate proven in pure tests; CLI parse rejects invalid levels. No `Command` test that a real `unique-pst … --fail-on-export-risk not_export_ready` returns status 65 with matching `exit_code`.  

**Impact:** Wiring regression would only show if someone disconnects `fail_on_export_risk` from `classify_export`.  

**Disposition:** Optional integration test; not a lock violation.

---

### [P3] Quarantine filename form differs from original spec prose (docs match code)
**Confidence:** High  

**Where:** `unique_pst_cmd.rs:847-855`  

**Spec prose:** `<out>.cancelled-<utc-timestamp>.pst.partial`  
**Implementation:** `{full_filename}.cancelled-{unix_secs}.partial` e.g. `unique.pst.cancelled-1710000000.partial`  
**Docs:** `docs/unique-pst-export.md` documents implementation form.  

**Impact:** Functional requirements met (not loadable as normal PST, `--out` freed, multi-volume). Timestamp is unix seconds, not ISO UTC.  

**Disposition:** Accept / document residual; optional rename to match original prose.

---

### [P3] Hard-fail with bytes on disk maps `artifact_state` to `partial_retained`
**Confidence:** Medium  

**Where:** `export_outcome.rs:artifact_state_for` Failed + `bytes_written` → `PartialRetained`  

**Spec table:** `partial_retained` is described as message-complete soft-fail (exit 64). Hard-fail incomplete exports can also retain bytes and get the same label.  

**Impact:** Orchestrators keying only on `artifact_state` without `fidelity`/`exit_code` could over-trust a hard-failed file. Correct consumers use the full contract.  

**Disposition:** Residual polish — consider `partial_retained` only for Partial fidelity, else a clearer failed-retained state (would need vocab amendment).

---

### [P3] DoD-19 multi-volume cancel + plain retry is unit-proven, not full mid-write E2E
**Confidence:** High  

**Where:** `quarantine_renames_primary_and_sibling`; cancel integration cancels pre-write (`AtomicBool` true immediately)  

**Impact:** Rename loop and free-`--out` covered; full “cancel mid multi-volume write then retry without `--overwrite` succeeds” process path not automated.  

**Disposition:** Accept with unit coverage; optional stress E2E.

---

### [P3] Process DoD-24 incomplete at audit time
**Confidence:** High  

**Where:** track folder missing final `review.md`; conductor/sequencing still **In Progress**; auditor did not re-run full workspace gate  

**Impact:** Track cannot be marked Completed until orchestrator writes `review.md` and records gate evidence. Does not reverse code locks.  

**Disposition:** Expected at subagent-r1; close on final gate.

---

No **P0** / **P1** / **P2** findings. Locks and FOCUS items from the handoff are satisfied by code review.

---

## Completeness Sweep

| Search | Result |
|---|---|
| TODO / FIXME / stub / placeholder / `unimplemented!` in 0078 surface | **None** in `export_outcome.rs`, unique-pst attach/exit sites, or `export_exit_0078.rs` |
| D-0073-eml falsely closed | **No** — deferred.md narrowed; counter site comments residual |
| D-0045-02 closed | **No** — annotated only |
| `process::exit` added | **No** |
| Short-circuit `classify_export` reasons | **No** — collects all then picks exit |
| `compute_export_ok` tests rewritten | **No** — still call `compute_export_ok` only |
| Codes 3/4 reused for fidelity | **No** — Busy/JobFailed unchanged; partial is 64 |

---

## Wiring and Regression Review

### unique-pst exit path (end-to-end)

1. Write/scan/verify → `ExportOkInput` dimensions (attach soft-fail **only** via `attach_failed_total`, not `export_error`).
2. Optional cancel → `quarantine_cancelled_volumes` on all existing volumes → `QuarantineResult`.
3. `export_risk` (0077) + `RiskGate` from `--fail-on-export-risk` (default Off).
4. `classify_export(...)` → fidelity, exit, reasons, cancelled.
5. `artifact_state_for` + absolute `summary_path` → `UniqueExportSummary` written; `exit_code` set from `classified.exit.as_u8()`.
6. JSON: print summary; non-success → `AlreadyEmitted { exit: classified.exit }` (preserves 64/65/130).
7. Human library path: `Ok(structured)` with `exit`; CLI `run_unique_pst` returns `Ok(outcome.exit)` and prints `summary: <abs>` to stderr on non-zero.
8. `main`: `Ok(code) => code.into()`; `Err(e) => e.exit_code().into()`.

### unique-eml exit path

1. `attach_parts_failed` incremented from `write_canonical_eml` soft fails.
2. `ExportOkInput` with attach soft vs write_errors/count hard dimensions.
3. Same `classify_export`; JSON `exit_code` / `AlreadyEmitted`; human `Ok(classified.exit)` + stderr summary line.

### GUI logical compile

- `UniquePstCliArgs` gained fidelity/risk fields; wizard sets `fail_on_partial_fidelity: true`, `allow_partial_fidelity: false`, `fail_on_export_risk: None`.
- `UniquePstOutcome` exit/fidelity fields mapped into `UniqueOutcomeView` (`exit_code`, `fidelity` `#[allow(dead_code)]` — D-0078-gui residual).
- Worker still uses `run_unique_pst_with_options`; no dual path. Partial/cancel still surface via `ok` / `cancelled` / export_risk banner (0077).

### Regression risks checked

| Risk | Result |
|---|---|
| Partial → exit 1 via `export.error` | **Blocked** by design at attach soft-fail site |
| Allow-partial without flag | Default fail-on remains true when neither flag |
| Risk gate default non-zero | Default Off; refinement preserved |
| Cancel leaves deliverable at `--out` | Quarantine rename; failed rename → `invalid_in_place` |
| 0–5 renumber | Not done |

---

## Verification Evidence

| Check | Status |
|---|---|
| Static review of classifier, unique-pst/eml/keep-set wiring, GUI args | Done (this audit) |
| Unit tests present for classify matrix, refinement, cumulative, cancel, quarantine | Present in tree |
| Integration `export_exit_0078.rs` (0 / 64 / 2 / 130) | Present |
| Existing attach-fail non-zero (`unique_pst.rs`) still valid (asserts non-zero, not `==1`) | Compatible with 64 |
| `error.rs` 0–5 mapping tests | Present unmodified in intent |
| Auditor re-ran `cargo test -p pst-dedup-cli` / full workspace gate | **Not re-run** (read-only audit; implementer claim only) |
| `cargo check -p pst-dedup-gui` | **Not re-run**; field mapping reviewed as consistent |

---

## Deferred Candidates

Items already on `docs/deferred.md` (do not re-open):

| ID | Notes |
|---|---|
| D-0073-eml | Full unique-eml ledger CSV — **narrowed, not closed** |
| D-0045-02 | Cross-process cancel — annotated only |
| D-0078-retryable | `retryable: bool` JSON (not exit code); anti-retry-exit-5 |
| D-0078-gui | Desk fidelity / exit_reason surfacing |

Optional new residuals (P3 only; **not** required to block completion):

| Candidate | Notes |
|---|---|
| D-0078-eml-exit-test | Dedicated unique-eml attach soft-fail → 64 proof |
| D-0078-risk-e2e | Process-level `--fail-on-export-risk` → 65 |
| D-0078-artifact-failed-retained | Distinct `artifact_state` for hard-fail retained volumes |
| D-0078-quarantine-name | Align rename pattern with original `.cancelled-*.pst.partial` prose |

---

## Completion Decision

### Verdict: **PASS WITH DEFERRED P3**

### Why not FAIL
1. All **locks** and **FOCUS** constraints are met in code (0–5 frozen, refinement, cumulative reasons, quarantine all volumes, no `process::exit`, `compute_export_ok` tests intact, attach soft-fail not forced through hard `export.error`).
2. Primary automation contract (64 / 65 / 130, JSON fields, cancel quarantine, flags, docs, deferred updates) is implemented with unit + targeted integration proof on unique-pst.
3. Remaining items are **P3** proof polish / process closeout, not broken semantics.

### Why not plain PASS
1. DoD-24 (`review.md`, full gate record, conductor **Completed**) still open.
2. Secondary verification holes (unique-eml 64 test, process 65, full multi-volume cancel retry E2E) remain P3.
3. Minor artifact_state / quarantine naming nits.

### Minimum to mark track Completed
1. Orchestrator: final `review.md`, flip conductor/sequencing to Completed after gate.
2. Prefer: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace` (or documented package gate + residual).
3. Optional but recommended: one unique-eml partial-exit test and one process-level exit-65 case.

### Re-grade note
If a later audit finds attach soft-fail exiting **1** in process, or cancel leaving a deliverable at `--out` without `invalid_in_place`, escalate to **FAIL** (P0/P1). No such defect found in this pass.
