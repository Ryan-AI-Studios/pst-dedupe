# 0078 Plan — Unique Export Exit Codes & Automation Contract

Spec: [`spec.md`](spec.md). Verified against `3d693e5`.

## Locks (do not violate without amending the spec)

1. Codes `0`–`5` frozen — value, mapping, and tests (§2.3.1)
2. New codes in **64–113** only (§2.3.2)
3. **Refinement only** — nothing non-zero today may become zero, except via an explicit flag (§2.3.4/5)
4. JSON is the truth; the integer is a routing hint (§2.3.3)
5. `ok` retained, `ok == (fidelity == complete)` (§3.5)
6. One vocabulary: `complete|partial|failed` ↔ codes (§2.3.7)
7. Cancellation is not failure, and outranks all other signals (§2.3.8, §3.3)
8. Codes derive from data, never from log/stderr state (§2.3.9, 0077 rule 2)
9. Attach failure stays non-zero by default (§2.3.10, 0073 handoff)
10. No `process::exit`; no self-signalling for 130 (§3.6)
11. `exit_reason` is a closed vocabulary — no PST-derived strings (§3.5)
12. D-0073-eml and D-0045-02 are **narrowed/annotated, not closed** (§2.4)
13. **Quarantine, never delete**, a partial artifact; never leave one at `--out` silently (§2.3.11, §3.6)
14. **Precedence picks the integer, not the reason set** — `classify_export` must not short-circuit (§2.3.12, §3.3)

## Phase 0 — Baseline (before any edit)

- [ ] Record the observed exit code for each shipped class: success, generic, usage, busy, job-failed, matter-io
- [ ] Record current unique-pst exit for: clean fixture, forced attach fail, cancellation
- [ ] Write `baseline.md`; this is the evidence for lock 3 — it cannot be reconstructed after the change

## Phase 1 — Plumbing (the D4 fix)

- [ ] `run(cli) -> Result<CliExit>`; mechanical `Ok(()) → Ok(CliExit::Success)` at every return
- [ ] `main.rs:897-906` → `Ok(code) => code.into()`
- [ ] No behavior change yet — gate must be green here, with every existing exit identical to `baseline.md`

## Phase 2 — Outcome type

- [ ] `ExportFidelity`, `ExportOutcome`, `classify_export` (§3.1), pure and unit-tested
- [ ] Re-express `compute_export_ok` via `classify_export`; **do not modify its existing tests** — they are the back-compat guard
- [ ] Add `CliExit::PartialFidelity = 64`, `ExportRiskBlocked = 65`
- [ ] Precedence exactly as §3.3: cancelled → hard fail → risk → partial → ok
- [ ] Evaluate **all** conditions and collect every reason; precedence orders the vec and picks the integer only (lock 14)

## Phase 3 — unique-pst wiring

- [ ] Replace the `AlreadyEmitted { exit: Generic }` site (`unique_pst_cmd.rs:2285`) with the classified exit
- [ ] Cancellation path (`:889`) → 130, cancelled `summary.json` still written first
- [ ] Artifact and report dir retained on 64 — assert it, don't assume it

## Phase 3a — Artifact quarantine (the D7 fix)

- [ ] On cancel after any bytes written, rename each written volume → `<out>.cancelled-<utc-ts>.pst.partial`
- [ ] Cover **all** multi-volume siblings (`volume_path_for`), not just the primary
- [ ] Rename failure → `artifact_state: invalid_in_place` + path; **do not** report a clean cancellation
- [ ] `artifact_state` on every summary (closed vocabulary, §3.6), not only cancelled runs
- [ ] Assert `--out` is free after quarantine and a plain retry needs no `--overwrite` (this is the half that makes 130 actionable)

## Phase 4 — Flags

- [ ] `--fail-on-partial-fidelity` (default on) / `--allow-partial-fidelity`
- [ ] Both supplied → exit 2, explicit message (never a silent precedence rule)
- [ ] `--fail-on-export-risk <level>`, **default off** (lock 3 in the opposite direction)

## Phase 5 — JSON

- [ ] `fidelity`, `exit_code`, `exit_reason`, `artifact_state`, `summary_path` — all `#[serde(default)]`, additive
- [ ] `ok` unchanged; assert consistency with `fidelity` in a test
- [ ] Closed `exit_reason` and `artifact_state` vocabularies (§3.5, §3.6)
- [ ] Human mode: `summary: <abs path>` to **stderr** on non-zero exit (stdout stays pipe-clean)

## Phase 6 — unique-eml + keep-set

- [ ] Data-path attach counters for unique-eml (narrow half of D-0073-eml — ledger CSV stays deferred)
- [ ] Same `classify_export` for both; replace `unique_eml_cmd.rs:404` site
- [ ] Comment at the counter site naming D-0073-eml so the residual's owner finds it

## Phase 7 — Tests

- [ ] `classify_export` matrix over all outcome classes
- [ ] **Refinement assertion** (DoD-14) — every class non-zero today is non-zero after
- [ ] Integration test reading the **real process status** and asserting it equals `summary.json.exit_code` (DoD-9)
- [ ] Precedence: cancelled + attach-fail + risk → 130, `exit_reason == ["CANCELLED"]`
- [ ] Cumulative reasons: risk + attach → 65 with `["RISK_GATE", "ATTACH_SOFT_FAIL"]`
- [ ] Cancel mid-write on a multi-volume fixture → all volumes quarantined, retry without `--overwrite` succeeds
- [ ] Rename-failure simulation → `invalid_in_place`
- [ ] 0077 corrupt fixture + `--fail-on-export-risk` → 65; without flag → baseline-identical

## Phase 8 — Docs

- [ ] Matrix in README + `docs/unique-pst-export.md`, severity ordering stated before numeric ordering
- [ ] PowerShell dispatch example (no `&&`, no bash-isms — repo shell rules)
- [ ] Lead with "64 means the artifact exists and is message-complete"
- [ ] Document `artifact_state`, especially that `invalid_in_place` requires a purge before retry
- [ ] 0081 handoff: **no blanket retry-by-exit-code**; name `AuditChainBroken` as the case that must never be silently retried
- [ ] Cross-link 0081 runbook and 0080 QC

## Phase 9 — Registry + gate

- [ ] `deferred.md`: D-0073-eml narrowed, D-0045-02 annotated, add D-0078-retryable + D-0078-gui
- [ ] `conductor.md` + `sequencing.md` rows
- [ ] `review.md`
- [ ] `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`

## Suggested order

Phase 0 → 1 → 2 are strictly sequential; **Phase 1 is the enabling change** and must land behavior-neutral and provably identical to baseline before anything else moves. Phases 4–6 can interleave.

**Phase 3a ships with Phase 3, not after it.** Exit 130 without quarantine is a net negative: it tells a script the run was cancelled and invites the retry that then fails on the leftover `--out`, while the truncated PST sits at the deliverable path looking finished. If the track is cut short, the minimum coherent slice is Phases 0–3a plus the refinement assertion.

## Handoff

**Do:** baseline first; keep `compute_export_ok` and its tests; make Phase 1 provably inert; ship 130 and quarantine together.

**Do not:** renumber 0–5; reuse 3/4 for fidelity; default the risk gate on; let anything reach 0 without an explicit flag; put PST-derived strings in `exit_reason`; call `process::exit` or self-signal for 130; delete a partial artifact or leave one at `--out`; short-circuit `classify_export`; recommend blanket retry-by-exit-code; claim D-0073-eml or D-0045-02 closed.
