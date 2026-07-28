# Track Completion Audit — 0078 (re-review r2)

## Verdict: PASS WITH DEFERRED P3

Post-r1 fixes land cleanly: unique-eml-style attach soft-fail → 64 is unit-proven, hard-fail retained volumes use `invalid_in_place` (not `partial_retained`), quarantine naming is code/docs aligned, and the unique-pst attach-fail process test tightens to exit **64** when `fidelity == partial`. Locks and FOCUS items remain satisfied. Residual deferrals match the allowlist only (process DoD-24, process-level 65 E2E, multi-volume mid-write cancel E2E, D-0073-eml full ledger, D-0045-02, D-0078-retryable, D-0078-gui). One optional polish residual on unique-eml’s *inline* `artifact_state` (does not use shared `artifact_state_for`) is noted at P3 and does not reverse the unique-pst fix.

## Scope Reviewed

| Artifact | Path |
|---|---|
| Prior audit | `conductor/0078-UniqueExportExitCodes/review.subagent-r1.md` |
| Spec / plan / baseline / notes | `conductor/0078-UniqueExportExitCodes/{spec,plan,baseline,implementation-notes}.md` |
| Outcome classifier | `crates/pst-dedup-cli/src/export_outcome.rs` |
| Exit enum / mapping | `crates/pst-dedup-cli/src/error.rs` |
| Plumbing | `crates/pst-dedup-cli/src/main.rs` (`run → Result<CliExit>`, `Ok(code) => code.into()`) |
| unique-pst + quarantine | `crates/pst-dedup-cli/src/unique_pst_cmd.rs` |
| unique-eml counters + classify | `crates/pst-dedup-cli/src/unique_eml_cmd.rs` |
| keep-set JSON fields | `crates/pst-dedup-cli/src/keep_set_cmd.rs` |
| Integration tests | `crates/pst-dedup-cli/tests/export_exit_0078.rs`, `tests/unique_pst.rs` |
| Docs | `docs/unique-pst-export.md`, `README.md`, `docs/deferred.md` |

Read-only audit. Full workspace cargo gate **not** re-run by this auditor.

---

## r1 Findings Disposition

| r1 finding | Disposition | Evidence |
|---|---|---|
| **[P3] unique-eml DoD-12 lacks dedicated attach soft-fail → 64 proof** | **FIXED** | `export_outcome.rs` unit `unique_eml_style_attach_soft_fail_is_partial_64`: same input shape unique-eml feeds → `Partial` + `CliExit::PartialFidelity` (64) + `ATTACH_SOFT_FAIL` + `PartialRetained` |
| **[P3] Exit 65 unit-only; no process-level fixture** | **Still deferred (allowed)** | Pure tests `risk_gate_not_export_ready_65` / `risk_gate_off_no_65` + CLI parse; no `Command` E2E for status 65 |
| **[P3] Quarantine filename form vs original spec prose** | **Fixed (docs/comments aligned)** | Code: `{filename}.cancelled-{unix_secs}.partial`; comments at quarantine helpers; `docs/unique-pst-export.md` documents implementation form. Residual rename to `.cancelled-*.pst.partial` prose **not** required |
| **[P3] Hard-fail + bytes → `partial_retained`** | **FIXED (shared helper)** | `artifact_state_for`: `Failed` + `bytes_written` → `InvalidInPlace`; unit `artifact_state_hard_fail_with_bytes_is_invalid_in_place`; docs §artifact_state. See optional unique-eml sibling residual below |
| **[P3] Multi-volume cancel mid-write process E2E** | **Still deferred (allowed)** | Unit quarantine multi-volume rename + cancel integration (pre-write `AtomicBool`); full mid-write retry E2E absent |
| **[P3] Process DoD-24 (`review.md` / full gate / conductor Completed)** | **Still deferred (process)** | This is subagent-r2; final `review.md` + gate record + conductor flip remain orchestrator |

---

## Requirement and DoD Matrix (delta vs r1)

| DoD | Status | Notes vs r1 |
|---|---|---|
| **1** `classify_export` pure + unit matrix | **Met** | Unchanged core + new unique-eml-style + hard-fail artifact units |
| **2** `run → Result<CliExit>`; no `process::exit` | **Met** | `main.rs:907-929`; SIGINT comments only in `runner_util.rs` |
| **3** `compute_export_ok` retained; tests unmodified | **Met** | Re-expression via classify; tests still assert `compute_export_ok` only |
| **4** Codes 0–5 frozen | **Met** | `error.rs` Success…MatterIo; mapping tests 0/2/5; 64/65/130 additive only |
| **5** Exit 64 attach soft-fail; artifact retained | **Met** | No `export_error` on attach soft (`unique_pst_cmd` ~2103–2109); process tests assert 64 when partial |
| **6** Exit 65 opt-in risk gate | **Met (unit)** | Same as r1; process 65 still open (allowed P3) |
| **7 / 7a** Cancel 130; reasons `["CANCELLED"]` only | **Met** | `export_exit_0078::cancel_exit_130_and_summary` |
| **8** Hard fail exit 1 | **Met** | Hard dimensions → `CliExit::Generic` |
| **9** `summary.exit_code` == process | **Met (partial classes)** | 0 / 64 / 130 process-asserted; 65 not |
| **10** fidelity fields unique-pst/eml/keep-set | **Met** | Shared summary + keep-set inserts + unique-eml payload |
| **11** allow-partial → 0; mutual exclusion → 2 | **Met** | Integration tests |
| **12** unique-eml data-path counters + partial | **Met (code + unit proof)** | Counters + `unique_eml_style_attach_soft_fail_is_partial_64` closes r1 proof gap (process unique-eml E2E still optional) |
| **13–16** vocab / refinement / precedence / docs | **Met** | README + unique-pst-export severity-first + PS switch |
| **17–18** deferred.md + conductor | **Met (In Progress expected)** | Residuals present; completion flip orchestrator |
| **19** Cancel quarantine all volumes | **Met (unit)** | Multi-volume process E2E allowed deferred |
| **20** Cumulative risk+attach reasons | **Met** | Unit `cumulative_risk_and_attach` |
| **21** `artifact_state` closed vocab; hard-fail retained | **Met (unique-pst / shared)** | `InvalidInPlace` for hard-fail+bytes; rename-fail path unchanged |
| **22–23** summary_path abs; 0081 handoff | **Met** | Notes + docs anti-retry-5 |
| **24** `review.md` + full gate | **Unmet (process)** | Expected at subagent pass |

### Locked rules (spot-check)

| Lock | Status |
|---|---|
| 0–5 frozen; new 64/65/130 only | **Met** |
| Refinement only; reasons cumulative; no reason short-circuit | **Met** |
| Cancel outranks; not failure | **Met** |
| Attach soft-fail not via `export.error` hard path | **Met** |
| No `process::exit` for 130 | **Met** |
| D-0073-eml / D-0045-02 not falsely closed | **Met** |
| Quarantine never delete; `invalid_in_place` on rename fail | **Met** |
| `compute_export_ok` tests not rewritten | **Met** |

---

## Findings (P0–P3)

### No new P0 / P1 / P2

Locks, FOCUS items, and the four post-r1 code/doc fixes hold under static review.

### Allowed deferred P3 (unchanged)

#### [P3] Exit 65 has no process-level fixture assertion
**Confidence:** High  
**Where:** missing `export_exit_0078` / similar `Command` case for `--fail-on-export-risk`  
**Disposition:** Allowlisted residual; unit + parse wiring sufficient for completion policy.

#### [P3] DoD-19 multi-volume mid-write cancel + plain retry is unit-proven, not full process E2E
**Confidence:** High  
**Disposition:** Allowlisted residual.

#### [P3] Process DoD-24 incomplete at audit time
**Confidence:** High  
**Where:** no final `review.md`; conductor/sequencing still In Progress; auditor did not re-run full workspace gate  
**Disposition:** Expected at subagent-r2; orchestrator closeout.

#### [P3] Deferred product residuals (already on deferred.md)
| ID | Status |
|---|---|
| D-0073-eml | Narrowed, not closed |
| D-0045-02 | Annotated only |
| D-0078-retryable | Open |
| D-0078-gui | Open |

### Optional residual (not allowlisted hard blocker)

#### [P3] unique-eml inline `artifact_state` still maps hard-fail + written EMLs → `partial_retained`
**Confidence:** Medium  

**Where:** `unique_eml_cmd.rs` ~413–420 — does **not** call `artifact_state_for`; uses  
`ok → Complete`, else `(Partial \|\| eml_written > 0) → PartialRetained`, else `Absent`.

**Evidence:** Shared helper correctly maps `Failed` + bytes → `InvalidInPlace` (r1 fix). unique-eml hard-fail (count mismatch / write_errors) with files on disk still labels `partial_retained`, which docs reserve for soft-fail deliverables.

**Impact:** Low — `fidelity` / `exit_code` remain honest (`failed` / 1). Risk only if consumers ignore those and key solely on `artifact_state`. EML packs are multi-file (no single PST quarantine path).

**Disposition:** Optional polish — route unique-eml through `artifact_state_for` (or mirror Failed → InvalidInPlace / Absent). **Does not** reverse unique-pst fix or locks. Not treated as FAIL.

---

## Completeness Sweep

| Search | Result |
|---|---|
| TODO / FIXME / `unimplemented!` on 0078 surface | **None** in classifier / unique-pst attach-exit / export_exit tests |
| D-0073-eml / D-0045-02 falsely closed | **No** |
| `process::exit` added | **No** |
| Short-circuit `classify_export` reasons | **No** — collect then pick integer |
| `compute_export_ok` test bodies rewritten | **No** |
| Codes 3/4 reused for fidelity | **No** — partial is 64 |
| Attach soft → exit 1 via `export.error` | **Blocked** at unique-pst attach site |

---

## Fresh Regression Sweep

### `export_outcome.rs`

- Integer precedence: cancelled → hard → risk → partial(fail_on) → 0.
- Reasons cumulative (hard → risk → soft); cancel suppresses to `["CANCELLED"]` only.
- `unique_eml_style_attach_soft_fail_is_partial_64` present and asserts 64 + PartialRetained.
- `artifact_state_hard_fail_with_bytes_is_invalid_in_place` present; cancelled paths unchanged.
- Refinement assertion still requires non-zero for former-nonzero classes.

### unique-pst exit path

1. Dimensions → `ExportOkInput` (attach soft **only** via `attach_failed_total`).
2. Cancel → `quarantine_cancelled_volumes` all volumes → `QuarantineResult`.
3. `export_risk` + `RiskGate` → `classify_export`.
4. `artifact_state_for` + absolute `summary_path` → summary `exit_code`.
5. JSON non-success → `AlreadyEmitted { exit: classified.exit }` (preserves 64/65/130).
6. Human: `Ok(outcome.exit)` + stderr `summary: <abs>` on non-zero.
7. Attach fail process test: if `fidelity == "partial"` then **code == 64** and JSON `exit_code == 64`.

### unique-eml

1. `attach_parts_failed` from write path → `attach_failed_total`.
2. Same `classify_export`; JSON `exit_code` / `AlreadyEmitted`; human `Ok(classified.exit)`.
3. Unit proof of partial/64 input shape (DoD-12).
4. Inline `artifact_state` residual as above.

### `main` / `error.rs` 0–5

- `fn run(cli: Cli) -> Result<CliExit>`; `Ok(code) => code.into()`; `Err(e) => e.exit_code().into()`.
- `CliExit`: 0 Success, 1 Generic, 2 Usage, 3 Busy, 4 JobFailed, 5 MatterIo, 64 PartialFidelity, 65 ExportRiskBlocked, 130 Cancelled.
- Mapping tests for MatterIo / Usage unchanged in role; `AlreadyEmitted` carries classified exit.

---

## Verification Evidence

| Check | Status |
|---|---|
| Static review of r1 fix sites + classifier + wiring | **Done** |
| Unit tests for unique-eml-style 64 + hard-fail InvalidInPlace | **Present** |
| Integration 0 / 64 / 2 / 130 | **Present** |
| unique_pst attach fail tightened for 64 when partial | **Present** |
| Auditor re-ran cargo tests / full gate | **Not re-run** (read-only) |

---

## Completion Decision

### Verdict: **PASS WITH DEFERRED P3**

### Why not FAIL
1. All locks and FOCUS constraints still hold; attach soft-fail does not hard-fail as exit 1.
2. All four post-r1 fixes verified in code/tests/docs.
3. Remaining gaps are allowlisted P3s (plus one optional unique-eml artifact_state polish).

### Why not plain PASS
1. DoD-24 process closeout open.
2. Process-level exit **65** fixture still missing.
3. Multi-volume mid-write cancel + retry E2E still missing.
4. Product deferreds D-0073-eml / D-0045-02 / D-0078-retryable / D-0078-gui remain open by design.

### Minimum to mark track Completed
1. Orchestrator: final `review.md`, conductor/sequencing → Completed after gate evidence.
2. Prefer full gate: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`.
3. Optional: process-level 65; unique-eml hard-fail → `invalid_in_place` via shared helper.

### Re-grade note
Escalate to **FAIL** if attach soft-fail exits **1** under partial fidelity, cancel leaves a silent deliverable at `--out` without `invalid_in_place`, or codes 0–5 are renumbered. No such defect found in r2.
