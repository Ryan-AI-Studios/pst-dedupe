# 0078 — Unique Export Exit Codes & Automation Contract

- **Track ID:** 0078-UniqueExportExitCodes
- **Status:** In Progress
- **Series:** L
- **Upstream:** 0071 (unique-pst CLI + `ok`), 0073 (attach ledger; handed exit taxonomy here), 0074 (preflight), 0077 (`export_risk`)
- **Blocks / feeds:** 0081 (operator runbook documents the matrix), 0080 (QC scripts branch on the codes)
- **Verified against:** `3d693e5` (post-0077 merge)

## 1. Objective

Give automation a **stable, lossless-enough outcome contract**. Today every non-success ending of `unique-pst` — a missing attachment, a count mismatch, a failed verify, an operator pressing Ctrl-C — arrives at the caller as the single integer `1`. The information needed to tell these apart already exists and is already written to `summary.json`; it is destroyed at the process boundary.

This track does not add new judgments about export quality. 0073/0074/0075/0076/0077 made those judgments. 0078 does one thing: **stop flattening them on the way out.**

| Capability | Today | After 0078 |
|---|---|---|
| Complete export | exit 0 | exit 0 (unchanged) |
| Messages complete, attachments soft-failed | exit **1** | exit **64**, artifact retained |
| `export_risk = not_export_ready` (0077) | exit 0 **or** 1, depending on unrelated attach luck | exit **65** under opt-in gate |
| Operator cancelled (Ctrl-C) | exit **1** | exit **130** (SIGINT convention) |
| Truncated PST left by a cancel | indistinguishable from a deliverable; blocks retry | **quarantined** + `artifact_state` in JSON |
| Artifact absent / untrustworthy | exit 1 | exit 1 (unchanged) |
| `fidelity` in JSON | absent | `complete\|partial\|failed` + `exit_code` + `exit_reason` + `artifact_state` + `summary_path` |
| unique-eml fidelity | not computable (counts/tracing only) | same contract, same codes |

### Industry anchors

- **Reserved ranges.** `1`, `2`, `126`, `127`, `128+n`, `255` carry shell-assigned meanings; the Advanced Bash-Scripting Guide recommends confining application-defined codes to **64–113** (the `sysexits.h` band). New codes here land in that band — not at `3`/`4` as the placeholder proposed (§2.2 D2).
- **Success-but-notable is a real pattern.** `rclone` exits **9** for "completed, transferred nothing" under `--error-on-no-transfer`, and **8**/**10** for limit-reached. Precedent for an exit that means *done, but the caller should care*.
- **SIGINT is 130** (`128 + 2`) by universal convention. Cancellation is not failure.

## 2. Context

### 2.1 What exists today (read at `3d693e5`)

- `CliExit` (`crates/pst-dedup-cli/src/error.rs:9-22`) ships six codes: `Success=0`, `Generic=1`, `Usage=2`, `Busy=3`, `JobFailed=4`, `MatterIo=5`. Mapping is tested (`error.rs:198-222`).
- `run(cli) -> Result<()>` (`main.rs:920`). `main` (`main.rs:897-906`) maps `Ok(())` → 0 and `Err(e)` → `e.exit_code()`. **There is no third shape.**
- `compute_export_ok` (`unique_pst_cmd.rs:2466-2474`) reduces eight independent dimensions — `scan_ok`, `verify_ok`, `export_err_absent`, `export_partial`, `messages_written_total == unique`, `attach_failed_total == 0`, `report_ok` — to one `bool`.
- That bool becomes `CliError::AlreadyEmitted { exit: CliExit::Generic }` (`unique_pst_cmd.rs:2285-2288`); unique-eml does the same (`unique_eml_cmd.rs:404-407`).
- `cancelled` is fully modelled: summary field (`unique_pst_cmd.rs:324`), a dedicated minimal-summary writer (`:736`), error code `"cancelled"` (`:844`), and a gate at `:889` (`outcome.ok && !outcome.cancelled`).
- `export_risk` (0077) reaches stdout (`unique_pst_cmd.rs:2317`) and `summary.json`.

**Operator evidence (INC0102784, 2026-07-26):** exit **1**, `ok: false`, **3728/3728** messages written, verification `open_ok`, **366** attach failures. The PST was usable and complete at the message level. The exit code said the run failed.

### 2.2 Defects

**D1 — One bit of outcome.** Every failure mode collapses to exit 1. A script cannot distinguish "retry this" from "review this" from "the artifact does not exist," so the only safe automation is to treat all of them as fatal — which is why the INC export, a usable 3728-message PST, had to be triaged by hand.

**D2 — The placeholder's own numbering collides.** The prior spec proposed `3` = partial fidelity and `4` = hard fail. `Busy = 3` and `JobFailed = 4` are already shipped and tested. Exit 3 today means *matter busy, retry in a minute*; the proposal would additionally make it mean *export finished, go inspect the attachments*. Those demand opposite automation responses from the same integer. **Rejected** — see §3.3.

**D3 — Cancellation reads as failure.** Everything needed to report cancellation honestly is computed and written to `summary.json`, then discarded at `main.rs`. Ctrl-C is indistinguishable from a corrupt source. Scripts retry what they should abandon and escalate what they should simply rerun.

**D4 — The signature is the cage.** Because `run` returns `Result<()>`, the *only* way to produce a non-zero exit is to construct an error. "Succeeded, with something to review" has no representable form, so it must impersonate a failure. D1 is structural, not a policy choice — and no amount of code-table design fixes it without changing this signature.

**D5 — 0077's risk verdict does not reach automation.** `export_risk` is computed, printed, and serialized, but never consulted for the exit code. A source flagged `not_export_ready` exits **0** if its attachments happened to all write. The single most safety-relevant signal the tool produces is invisible to the layer that acts on it.

**D6 — unique-eml cannot compute its own fidelity.** unique-pst has the 0073 ledger; unique-eml soft-skips attachments with counts and `tracing` only (D-0073-eml). Since a `tracing` line is not data (0077 locked rule 2), unique-eml has nothing in the data path to base exit 64 on.

**D7 — A cancelled run leaves a truncated PST that looks like a deliverable.** Cancellation is checked at eight points, including inside the write loop itself (`unique_pst_cmd.rs:1655`, `while cursor < prepared.len() && !cancelled`). Nothing removes or marks the partial output. Two consequences, both verified:

- **Mistaken production.** A truncated PST sits at the exact `--out` path a completed export would occupy, with the same extension and no marker. An operator who did not run the command cannot tell it apart from a finished deliverable, and a review tool will happily load it. That is a silently short production — the same failure class 0077 addressed for ScanPST deletions.
- **Retry is blocked.** `:983` refuses an existing `--out` without `--overwrite`. A script that receives 130 and retries gets **exit 2 (usage)**, not a clean rerun. The obvious operator workaround — always pass `--overwrite` — is worse, because it disarms the guard that protects a *good* export from being clobbered.

Giving automation a precise cancellation signal (D3) while leaving a booby-trapped artifact behind would make the pipeline more confident and no safer.

### 2.3 Locked rules

1. **Never renumber a shipped code.** `0`–`5` are frozen, including their `CliError` mappings and tests.
2. **New codes live in 64–113.** Per the ABS/`sysexits.h` recommendation, clear of shell-reserved ranges.
3. **The exit code is a lossy projection of `summary.json`, never the source of truth.** Anything an operator or script must reason about in detail lives in JSON. The integer is a routing hint.
4. **Refinement, not renumbering.** New codes may only subdivide the current exit-**1** bucket. Every outcome that exits non-zero today still exits non-zero. This is the exit-code analogue of 0076's split-only discipline, and it is asserted in tests (DoD-14) the same way.
5. **Only an explicit flag may move an outcome into 0.** No default loosening, ever.
6. **Exit 0 is honest.** It means the artifact is complete *and* there is nothing awaiting operator review.
7. **One outcome vocabulary.** `fidelity: complete|partial|failed` maps 1:1 onto the codes. No second parallel vocabulary (the mistake 0076 had to delete).
8. **Cancellation is not failure.** It is its own terminal state.
9. **Codes derive from data, not from log or stderr state** — inherited from 0077 locked rule 2.
10. **Attach failure stays non-zero by default** — the constraint 0073 explicitly handed to this track.
11. **No artifact may be indistinguishable from a deliverable unless it is one.** Every terminal state names its artifact in `artifact_state`, and an incomplete PST is moved off the deliverable path (§3.6). An exit code that is honest about a run that left a dishonest file on disk has not solved the problem.
12. **`exit_reason` is cumulative; only the integer is a winner.** All observed conditions are recorded, even when a higher-precedence condition sets the code (§3.3). The exception is cancellation, which suppresses *finding*-style reasons because the run stopped observing (§3.6).

### 2.4 Rolled-in deferred items

| Item | Disposition |
|---|---|
| **D-0073-eml** (P2) — unique-eml attach ledger parity | **Partially rolled in.** 0078 needs only the *data-path counters* (attach attempted / written / failed) so unique-eml can compute `fidelity` honestly; the full ledger-CSV parity stays deferred to 0073's residual. Scoped narrowly and stated as such so the residual is not falsely closed. |
| **0073 handoff** — "exit code taxonomy → 0078; keep 0071 attach→non-zero" | **Honored** as locked rule 10. |
| **D-0045-02** — cross-process cancel of an in-flight job | **Not closed.** 0078 makes in-process cancellation *observable* (exit 130); cross-process cancel remains deferred. Noted here only to prevent a later reader assuming 0078 closed it. |

## 3. Design

### 3.1 The outcome type

A single pure function produces the outcome; nothing else decides an exit code.

```rust
/// Terminal fidelity of an export operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFidelity { Complete, Partial, Failed }

/// What the process should tell its caller.
#[derive(Clone, Debug)]
pub struct ExportOutcome {
    pub fidelity: ExportFidelity,
    pub exit: CliExit,
    /// Stable machine codes, worst-first; never PST-derived strings (0077 DoD-13).
    pub reasons: Vec<&'static str>,
    pub cancelled: bool,
}

pub fn classify_export(i: ExportOkInput, risk: PreflightRecommendation, gate: RiskGate) -> ExportOutcome;
```

`classify_export` **extends** `compute_export_ok` (`unique_pst_cmd.rs:2466`) rather than replacing it: `compute_export_ok` stays and is re-expressed as `classify_export(..).fidelity == Complete`, so its existing tests keep their meaning and become the back-compat guard for rule 4.

### 3.2 Plumbing (the D4 fix — do this first)

Change `run(cli) -> Result<()>` to **`run(cli) -> Result<CliExit>`**, and `main.rs:897-906` to `Ok(code) => code.into()`. Every existing `Ok(())` becomes `Ok(CliExit::Success)` — mechanical, and it makes "success with a code" representable. Without this, §3.3 cannot be implemented without abusing the error path, which is exactly how the tool arrived at D1.

### 3.3 The code table

| Code | Name | Meaning | Script action |
|---|---|---|---|
| 0 | `Success` | Complete fidelity; nothing to review | proceed |
| 1 | `Generic` | Hard fail — artifact absent or untrustworthy | investigate; do not ship |
| 2 | `Usage` | Bad arguments *(frozen)* | fix the invocation |
| 3 | `Busy` | Matter busy *(frozen)* | retry later |
| 4 | `JobFailed` | Job failed/cancelled *(frozen)* | investigate |
| 5 | `MatterIo` | Matter open/IO *(frozen)* | investigate |
| **64** | `PartialFidelity` | **Artifact written and message-complete; attachment/body soft-failures recorded.** Retained. | review ledger; ship only with disclosure |
| **65** | `ExportRiskBlocked` | 0077 `export_risk` met the configured gate | re-export from source; do not produce |
| **130** | *(SIGINT)* | Operator cancelled | rerun; not an error |

Hard fail stays **1**. New codes carve *out of* the exit-1 bucket only (rule 4), so `if ($LASTEXITCODE -ne 0)` keeps its exact present meaning and no existing script breaks.

**Precedence** (deterministic, worst-first): cancelled → 130; hard fail → 1; risk gate met → 65; partial fidelity → 64; else 0.

Precedence selects **the integer only**. `classify_export` evaluates *every* condition and pushes each one it observes onto `reasons`; it must not short-circuit. A run that trips the risk gate and also soft-failed attachments exits **65** and reports `["RISK_GATE", "ATTACH_SOFT_FAIL"]` — losing the second entry would hide a real finding behind an unrelated one and quietly break locked rule 3 (JSON is the truth). Reasons are emitted in precedence order so the first entry always explains the integer.

Cancellation is the one exception (§3.6): it suppresses finding-style reasons, because a cancelled run stopped observing and its partial counts are not findings.

### 3.4 Flags

| Flag | Default | Effect |
|---|---|---|
| `--fail-on-partial-fidelity` | **on** | Partial → 64 (non-zero). Preserves rules 5 and 10. |
| `--allow-partial-fidelity` | off | Partial → **0**, `fidelity` still `partial` in JSON. The one sanctioned way into 0. |
| `--fail-on-export-risk <ok\|re_export_recommended\|not_export_ready>` | **`off`** | Opt-in gate. When the run's `export_risk` rank ≥ the argument, exit 65. |

`--fail-on-export-risk` defaults to *disabled* rather than `not_export_ready`: enabling it by default would move runs that exit 0 today into a non-zero code, violating rule 4 in the opposite direction. 0081 recommends `not_export_ready` as the runbook default for legal-hold work; the tool does not impose it.

The two fidelity flags are mutually exclusive — supplying both is a **usage error (exit 2)**, not a silent precedence rule.

### 3.5 JSON contract

Added to the unique-pst / unique-eml / keep-set summaries (all `#[serde(default)]`, additive — 0076 precedent):

```json
{
  "ok": false,
  "fidelity": "partial",
  "exit_code": 64,
  "exit_reason": ["ATTACH_SOFT_FAIL"],
  "artifact_state": "partial_retained",
  "summary_path": "D:\\exports\\INC0102784\\report\\summary.json",
  "export_risk": { "level": "re_export_recommended" }
}
```

**Locating the summary.** `summary_path` carries the absolute path *of the file it appears in*, so a summary read from an archive, a log, or a pipe remains self-locating. The path itself is already deterministic — `report_dir` is either `--report-dir` or `default_report_dir(&out)` derived from `--out` (`unique_pst_cmd.rs:938-940`), and the summary is always `report_dir/summary.json` (`:1018`). This tool does not use timestamped output directories, so an orchestrator can always compute the path in advance; `summary_path` exists to make the artifact self-describing, not to rescue a caller who cannot find it.

In `--json` mode the entire summary is already printed to stdout (`:2280`), so no additional emission is needed. The gap is **human mode**: on any non-zero exit the absolute `summary_path` is printed as the final stderr line, prefixed `summary: `, so a wrapper script that did not pass `--json` can still route the failure. Human-mode detail goes to stderr rather than stdout to keep stdout clean for callers that pipe it.

`ok` is **retained and unchanged** (`ok == (fidelity == complete)`) so existing consumers keep working; `fidelity` is the richer successor. `exit_reason` uses a closed vocabulary of stable codes — `ATTACH_SOFT_FAIL`, `BODY_SOFT_FAIL`, `COUNT_MISMATCH`, `VERIFY_FAILED`, `REPORT_WRITE_FAILED`, `SCAN_FAILED`, `RISK_GATE`, `CANCELLED` — and never interpolates a path, subject, or any PST-derived string.

`exit_code` in the JSON **must equal** the actual process exit status. This is a DoD assertion (DoD-9), not a convention: a summary that reports one code while the process returns another is worse than no field at all.

### 3.6 Cancellation → 130, and the artifact it leaves behind

`main` returns `ExitCode::from(130)` when the outcome is cancelled. The existing SIGINT handler is untouched — it still only sets a flag and never calls `process::exit` (`runner_util.rs:23-36`), so the cancelled `summary.json` is still written before exit. 130 is produced by the **normal return path**, not by self-signalling.

**Artifact disposition (the D7 fix).** On cancellation after any bytes have been written, the CLI **quarantines** rather than deletes: each written volume is renamed from `<out>.pst` to `<out>.cancelled-<utc-timestamp>.pst.partial`.

Quarantine is chosen over deletion deliberately:

| Option | Verdict |
|---|---|
| Leave in place | **Rejected** — this is D7. Looks like a deliverable; blocks retry. |
| Delete | **Rejected.** Destroys up to hours of work on a keystroke, and destroys the diagnostic evidence for *why* a run was cancelled. A tool whose stated discipline is "honest partial results over silent loss" should not respond to an interrupt by deleting the operator's data. |
| **Quarantine (rename)** | **Chosen.** The `.partial` suffix means no review tool loads it as a PST and no human mistakes it for output; the `--out` path is freed so a plain retry succeeds without `--overwrite`; the bytes survive for diagnosis. |

Rename is cheap and atomic within a volume, so it is safe on the interrupt path. If it fails (file locked, e.g. by AV), the run **must not** report a clean cancellation: `artifact_state` records `invalid_in_place` with the path, and the runbook instructs the orchestrator to purge before retrying. Multi-volume exports quarantine every sibling produced by `volume_path_for`, not just the primary.

`artifact_state` is a closed vocabulary on every summary, not only cancelled ones:

| Value | Meaning |
|---|---|
| `complete` | Artifact at `--out` is the full deliverable |
| `partial_retained` | Message-complete, soft failures recorded (exit 64) — a deliverable, with disclosure |
| `partial_quarantined` | Incomplete; renamed to `.partial`; `--out` free for retry |
| `invalid_in_place` | Incomplete and **still at `--out`**; quarantine failed; must be purged |
| `absent` | Nothing written |

`invalid_in_place` is the only state that requires orchestrator action before a retry, which is precisely why it gets its own value instead of being folded into a boolean.

Cancelled runs report `exit_reason: ["CANCELLED"]` only. Attach and CRC counters remain in the summary as raw numbers, but are not promoted to reason codes: a run that was interrupted at 40% did not observe an attachment failure *rate*, and reporting one as a finding would invite a re-export decision based on a sample the operator never chose.

### 3.7 Scope of application

`unique-pst`, `unique-eml`, and `keep-set` share `classify_export`. `scan` is unchanged (it is a report, not an export). Matter/service/platform subcommands are untouched — their codes are frozen by rule 1.

### 3.8 Docs

The matrix ships in `docs/unique-pst-export.md` and README with a copy-pasteable PowerShell dispatch example, and is cross-linked from 0081's runbook. Documentation states plainly that **64 means the artifact exists and is message-complete** — the failure mode this track exists to end is an operator deleting a good export because the shell said `1`.

## 4. Out of scope

- Changing matter service HTTP status codes.
- Renumbering or re-mapping codes 0–5.
- New quality judgments — 0078 reports existing verdicts, it does not compute new ones.
- Full unique-eml attach **ledger CSV** parity (stays D-0073-eml).
- Cross-process cancel (stays D-0045-02).
- **New residual — D-0078-retryable:** transient (retry-safe) vs permanent failure. Real problem — PSTs read over SMB or cloud mounts hit network drops and AV-induced file locks constantly, and a pipeline that halts a human on every one is unusable. But the signal should be a **`retryable: bool` in JSON, not a new exit code**: retryability cross-cuts outcome classes (a transient IO can surface as exit 1 during scan or exit 5 during matter open), so encoding it in the integer would require doubling the code table. Deferred because the taxonomy across `PstError`/`matter_core::Error` does not exist yet.

  **What 0081 must not say** is "treat exit 5 as retryable." `CliExit::MatterIo` covers `Io` and `Sqlite` — plausibly transient — but also `AuditChainBroken`, `SchemaVersionMismatch`, `WrongPassphrase`, and `DatabaseMissing` (`error.rs:144-162`), where retrying is useless and, for a broken audit chain, actively harmful: it delays escalation of the most serious integrity failure the tool can detect behind three rounds of backoff. Blanket per-code retry advice is what the `retryable` field exists to replace.
- **New residual — D-0078-gui:** Desk surfacing of `fidelity`. The 0077 banner already covers `export_risk`, which is the safety-critical half.

## 5. Preconditions

- 0077 merged (`export_risk` on `PreflightRecommendation`) — satisfied at `3d693e5`.
- Baseline capture **before any edit**: current exit code for each of the six shipped classes, recorded in `baseline.md`, to prove rule 4 afterwards.

## 6. Risks

| Risk | Mitigation |
|---|---|
| A script keys on `== 1` for attach failure and now sees 64 | Documented as the intended, breaking-by-design refinement; `--fail-on-partial-fidelity` keeps it non-zero; called out in review notes and 0081 |
| Exit 64 read as "worse than 1" because the number is larger | Docs lead with severity ordering, not numeric ordering; `exit_reason` carries meaning |
| `--allow-partial-fidelity` becomes a habitual mute | JSON still says `partial`; 0081 runbook warns; flag is never defaulted on |
| `exit_code` field drifts from the real exit status | DoD-9 asserts equality end-to-end in an integration test, not a unit test |
| Windows exit codes > 255 or negative | All codes ≤ 130 and `u8`; `ExitCode::from(u8)` only |
| 130 collides with a genuine child-process signal exit | The CLI never proxies a child's status into its own; 130 is emitted only from the cancelled branch |
| unique-eml counters undercount and produce a false `complete` | Counters land in the data path with a test that a forced skip yields `partial` (DoD-12) |
| Quarantine rename fails (AV/file lock) and the truncated PST stays at `--out` | `artifact_state: invalid_in_place` + non-clean cancellation reporting; runbook mandates purge before retry (DoD-21) |
| Operators start passing `--overwrite` habitually to work around cancelled leftovers | Quarantine frees `--out`, so plain retry works; this is the reason quarantine beats leave-in-place |
| `.partial` files accumulate and fill the export volume | Timestamped names never collide; retention is an operator/runbook concern, called out in 0081 |
| A short-circuiting `classify_export` hides a finding behind a higher-precedence one | Cumulative `reasons` asserted for the risk+attach combination (DoD-20) |
| 0081 gives blanket "retry exit 5" advice and buries an `AuditChainBroken` | Stated as an explicit anti-recommendation in §4 and carried into the 0081 handoff |

## 7. Definition of Done

1. [ ] `ExportFidelity`, `ExportOutcome`, `classify_export` land as pure, unit-tested code.
2. [ ] `run(cli) -> Result<CliExit>`; `main` maps it; no `process::exit` added.
3. [ ] `compute_export_ok` retained, re-expressed via `classify_export`, all existing tests green **unmodified**.
4. [ ] Codes 0–5 unchanged in value, mapping, and tests.
5. [ ] Exit 64 on message-complete + attach-soft-fail; artifact and report dir retained.
6. [ ] Exit 65 only when `--fail-on-export-risk` is supplied and the rank is met.
7. [ ] Exit 130 on cancellation, with the cancelled `summary.json` written first.
7a. [ ] Cancelled `exit_reason` is `["CANCELLED"]` only; raw counters retained, not promoted to findings.
8. [ ] Hard fail still exits 1 with no artifact lie.
9. [ ] **`summary.json.exit_code` equals the observed process exit status** — asserted in an integration test that reads the real status.
10. [ ] `fidelity` / `exit_code` / `exit_reason` present in unique-pst, unique-eml, keep-set JSON; `ok` unchanged and consistent with `fidelity`.
11. [ ] `--allow-partial-fidelity` → exit 0 with `fidelity: partial`; mutually exclusive with `--fail-on-partial-fidelity` → exit 2.
12. [ ] unique-eml computes fidelity from data-path counters; forced attach skip yields `partial` (D-0073-eml, narrow half).
13. [ ] `exit_reason` closed vocabulary; no PST-derived strings (0077 DoD-13 parity).
14. [ ] **Refinement assertion:** a table test over every outcome class proves each one that exits non-zero today still exits non-zero (rule 4).
15. [ ] Precedence test: cancelled + attach-fail + risk → **130**.
16. [ ] Matrix documented in README + `docs/unique-pst-export.md` with a PowerShell dispatch example.
17. [ ] `deferred.md` updated: D-0073-eml narrowed (not closed), D-0045-02 annotated, D-0078-retryable + D-0078-gui added.
18. [ ] `conductor.md` / `sequencing.md` rows updated; cross-link 0081.
19. [ ] **Cancel mid-write quarantines every written volume** to `.cancelled-<ts>.pst.partial`; `--out` is free afterwards and a plain retry (no `--overwrite`) succeeds.
20. [ ] **Cumulative reasons:** risk gate + attach soft-fail exits **65** with `exit_reason == ["RISK_GATE", "ATTACH_SOFT_FAIL"]`.
21. [ ] `artifact_state` present on every summary, closed vocabulary; forced-rename-failure test yields `invalid_in_place` with the path.
22. [ ] `summary_path` absolute and self-consistent; human mode prints `summary: <path>` to stderr on non-zero exit.
23. [ ] 0081 handoff records the anti-recommendation against blanket per-code retry, naming `AuditChainBroken`.
24. [ ] `review.md` written; full cargo gate green.

## 8. Verification

1. `cargo test -p pst-dedup-cli` — `classify_export` matrix, precedence, mutual exclusion.
2. Integration test asserting real process status per class (covers DoD-9/14).
3. Existing `error.rs` mapping tests pass **unmodified** (DoD-4).
4. Fixture unique-pst, clean source → 0, `fidelity: complete`.
5. Fixture with a forced attach failure → 64; rerun with `--allow-partial-fidelity` → 0, JSON still `partial`.
6. Fixture with 0077 CRC-corrupt source + `--fail-on-export-risk not_export_ready` → 65; without the flag → unchanged from baseline.
7. Cancellation test → 130 and a readable cancelled `summary.json`.
7a. **Cancel mid-write** on a multi-volume fixture → every volume quarantined, `--out` free, immediate retry without `--overwrite` succeeds (DoD-19).
7b. Rename-failure simulation → `artifact_state: invalid_in_place` (DoD-21).
8. unique-eml forced skip → 64 (DoD-12).
8a. Risk gate + attach soft-fail → 65 with both reason codes present and `RISK_GATE` first (DoD-20).
9. `baseline.md` diff review confirming rule 4 across all six shipped classes.
10. Full gate: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`.

## 9. Handoff

**Do:** change the `run` signature first; keep `compute_export_ok`; capture the baseline before editing; treat the code table as the contract and JSON as the truth.

**Do not:**
- Renumber 0–5, or reuse 3/4 for fidelity (D2).
- Default `--fail-on-export-risk` to anything but off.
- Let any outcome that is non-zero today become 0 without an explicit flag.
- Put a path, subject, or filename in `exit_reason`.
- Claim D-0073-eml or D-0045-02 as closed.
- Call `process::exit`, or self-signal to produce 130.
- Add exit codes outside 64–113 (excepting the frozen 0–5 and the conventional 130).
- **Delete** a partial artifact on cancel — quarantine it (§3.6).
- Leave a truncated PST at the `--out` path, or report a clean cancellation when quarantine failed.
- Short-circuit `classify_export` — precedence picks the integer, never the reason set.
- Promote a cancelled run's partial counters to reason codes.
- Recommend blanket retry-by-exit-code in 0081; retryability is `retryable` in JSON (D-0078-retryable).
