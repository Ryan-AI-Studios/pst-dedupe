# Track review: 0109-AlsoEmlClassifyHonesty

**Harness:** Antigravity (`agy`)  
**Track:** `conductor/0109-AlsoEmlClassifyHonesty`  
**Date:** 2026-08-29  

---

## Summary

Line-by-line verification of every Origin claim in `spec.md` against live code on `main` @ `f49857e`:

1. **Origin Bugbot Finding 1 (Fidelity derived from exit integer & `ok` broken across all unique-pst):**
   - *Verification:* Verified live in [`crates/pst-dedup-cli/src/unique_pst_cmd.rs:L3416-3427`](file:///C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs#L3416-L3427).
   - In lines 3416–3424, `classified.fidelity` is overwritten by matching on `combined_exit`: `Success => Complete`, `PartialFidelity => Partial`, `_ => Failed`.
   - When `--allow-partial-fidelity` is enabled, `combined_exit` is `CliExit::Success` (0), falsely mutating `fidelity` to `Complete`.
   - When `--fail-on-export-risk` trips, `combined_exit` is `CliExit::RiskGate` (65), falsely mutating `fidelity` to `Failed` even when extraction fidelity was 100% `Complete`.
   - In line 3427, `let ok = combined_exit == crate::error::CliExit::Success && !process_cancelled;` sets `ok: true` whenever exit is 0. In 0078, `ok` is defined as `fidelity == Complete && !cancelled` (so `--allow-partial-fidelity` with partial writes must report `exit: 0`, `ok: false`, `fidelity: partial`). This bug currently affects **all** `unique-pst` runs (with or without `--also-eml`).

2. **Origin Bugbot Finding 2 (Summary rewrite drops also-eml cancel 130):**
   - *Verification:* Verified live in [`crates/pst-dedup-cli/src/unique_pst_cmd.rs:L3550-3577`](file:///C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs#L3550-L3577).
   - On `summary.json` write failure, line 3555 passes `cancelled` (PST-only cancel flag) instead of `process_cancelled` (`cancelled || also_eml_cancelled`).
   - If cancel occurred during the `--also-eml` phase, `cancelled` is `false`, so `classify_export` sees `report_ok: false, cancelled: false`, converting exit `130` into exit `1` (Generic) and setting `retryable: false`.

3. **Origin Bugbot Finding 3 (Cancel Err->Ok conversion zeros counters):**
   - *Verification:* Verified live in [`crates/pst-dedup-cli/src/unique_eml_cmd.rs:L461-471`](file:///C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_eml_cmd.rs#L461-L471).
   - When pack write returns `Err` after cancellation, lines 464–466 hardcode `attach_parts_written: 0`, `attach_parts_failed: 0`, `embedded_messages_written: 0` instead of recovering counts via `also_eml_recovered_counts(out)`.

---

## Blind-Spot Headlines

1. **False-Pass Hazard in Allow-Partial Test Fixtures:** Clean test PSTs with 0 attachment failures will pass `--allow-partial-fidelity` tests trivially because `fidelity` starts as `Complete`. Tests must explicitly inject partial fidelity or test `finalize_unique_pst_classify` directly with `ExportFidelity::Partial`.
2. **PST `artifact_state` Isolation During Also-EML Cancel:** When an also-eml cancel occurs, combined fidelity becomes `Failed`, but the PST `--out` was already successfully written. Recomputing `artifact_state` from the combined result would falsely mark a valid PST as `invalid_in_place`.
3. **Recovery Pre-Seeding in Unit Tests:** Testing cancel count recovery without pre-seeding `summary.json` with non-zero integers creates a false pass (0 == 0).

---

## Findings (B/M/m/O)

| ID | Sev | Finding with concrete failure scenario | Fix |
|---|---|---|---|
| **F-0109-1** | **Major** | **`ok` and `fidelity` contract broken across ALL unique-pst jobs:** [`unique_pst_cmd.rs:L3416-3427`](file:///C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs#L3416-L3427) assigns `ok = (combined_exit == Success)` and maps `fidelity` from exit integers. With `--allow-partial-fidelity`, a job with 50 failed attachments returns `ok: true, fidelity: complete, exit: 0`, violating 0078 specifications. | Replace with `worse_export_fidelity(pst, eml)` and enforce `ok = (classified.fidelity == Complete) && !process_cancelled` across all unique-pst jobs. |
| **F-0109-2** | **Major** | **Also-eml cancel 130 clobbered to exit 1 on summary rewrite error:** [`unique_pst_cmd.rs:L3550-3577`](file:///C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs#L3550-L3577) passes PST-only `cancelled` instead of `process_cancelled`, wiping out exit 130 and reporting non-retryable error. | Pass `process_cancelled` to `classify_export` and `summary_is_retryable` in the summary rewrite handler. |
| **F-0109-3** | **Minor** | **Zeroed telemetry on cancelled EML pack write:** [`unique_eml_cmd.rs:L464-466`](file:///C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_eml_cmd.rs#L464-L466) hardcodes 0 for `attach_parts_failed` and `embedded_messages_written` on cancel `Err`->`Ok`. | Call `also_eml_recovered_counts(out)` to populate `WriteEmlPackFromKeepSetResult`. |
| **F-0109-4** | **Observational** | **PST `artifact_state` must remain decoupled from combined fidelity:** Combined `fidelity: failed` from also-eml cancel must not mutate `summary.artifact_state` for the PST `--out`. | Lock `artifact_state` computation to occur before combined exit/fidelity merging. |

---

## What Looks Solid

- **0078 Exit Code Precedence:** The plan preserves `worse_cli_exit` with `130 > 1 > 65 > 64 > 0` precedence.
- **Quarantine Isolation:** Complete isolation between PST output directory and also-eml output directory is maintained.
- **Oracle Anti-Stripping Discipline:** No summary keys are added to `SUMMARY_ALLOWLIST_KEYS`, preventing recursive wiping of verification telemetry.

---

## Deferred Fold-In Table

| Deferred ID | Action | Rationale |
|---|---|---|
| `D-0109-also-eml-classify` | **Absorb and close** | Fully resolved by Track 0109. |
| `D-0108-keepset-crc-retaint` | **Decline (keep open)** | P3 keep-set CRC re-taint residual from 0108. |
| `D-0067-embedded-depth` | **Decline (keep open)** | Matter / Relativity child-document extract residual. |
| `D-0100-hn-bitmap-hdr` | **Decline (keep open)** | Fail-closed HNBITMAPHDR residual. |

---

## PR / Review Comments the Plan Missed

- None. The plan explicitly absorbs all 3 Cursor Bugbot findings from PR #104.

---

## Research / Tools Notes

- **ai-brains:** Used from `C:\dev\Dedupe`. Preflight verified (3883 pinned memories). Query confirmed decision record `3a1fc687` for Track 0109.
- **ledgerful:** Used from `C:\dev\Dedupe`. Verified status `0 pending / 0 unaudited drift`.
- **gh cli:** Verified last merged PRs (#108, #107, #106, #105).

---

## Verdict: Ready after fixes

The plan is well-architected and ready to implement. Apply test rigor requirements to ensure tests do not pass accidentally on clean fixtures.

To fold in these review findings, run:
```powershell
/foldin 0109
```
