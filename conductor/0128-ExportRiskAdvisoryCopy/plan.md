# 0128 — ExportRiskAdvisoryCopy — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Ledger (execute):** `ledgerful ledger start 0128-export-risk-copy --category FEATURE --message "Complete unique export_risk note names BODY_UNAVAILABLE not poly CRC"`
>
> **Fold-in 2026-09-03:** opencode-M1/m1/m2/O1 + AGY-128-01..04.

## Phase 0 → DoD-4

- [ ] Re-read `compute_export_risk_with_thresholds`, `ExportRisk`, 0108 poly-only unit tests. Confirm 0.02 and three-value vocabulary.
- [ ] Do **not** discount `BODY_UNAVAILABLE`. Do **not** restrip keep-set CRC. Do **not** add a fourth risk level.

## Phase 1 → DoD-1, DoD-2, DoD-3

- [ ] Add `body_unavailable_winners: u64` (`#[serde(default)]`) on `ExportRiskInputs`; populate from winners before `compute_export_risk`.
- [ ] Add `operator_note: Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- [ ] Helper `export_risk_operator_note(fidelity, &ExportRisk) -> Option<String>`: complete + `ReExportRecommended` + **sole** elevating reasons = keyed degrade (± `poly_class_crc_discounted`). No `KeepSet` argument.
- [ ] stderr via `emit_log` before Phase 6 stdout/`--json`. Never `println!` the note.
- [ ] Runbook §5 split (poly CRC vs missing bodies). `--fail-on-export-risk` still gates `re_export_recommended`. Skip export-docs unless still undifferentiated.
- [ ] Units: poly-only complete → no note; keyed degrade > 0.02 with `attach_fail_rate` 0 → note names BODY_UNAVAILABLE; keyed degrade **plus** attach-fail → no note.

## Phase 2 → DoD-4, DoD-5

- [ ] `cargo test -p pst-dedup-cli export_risk`
- [ ] fmt / clippy / CHANGELOG / `review.md` / registry Completed / ledger commit
