# 0128 — ExportRiskAdvisoryCopy — Review

## Scope

Disclosure honesty for a structurally complete unique-pst whose `export_risk` is still `re_export_recommended` because keyed non-poly degrade (BODY_UNAVAILABLE share) exceeds 0.02. Additive `ExportRisk.operator_note` + `ExportRiskInputs.body_unavailable_winners`. Helper `export_risk_operator_note` is Some only if fidelity Complete, level ReExportRecommended, sole elevating reasons keyed degrade +/- `poly_class_crc_discounted`, AND `body_unavailable_winners > 0`. Note assigned AFTER `finalize_unique_pst_classify` (also-eml combine). Threshold 0.02 stays. No fourth ExportRisk enum. Do not discount BODY_UNAVAILABLE. No BCC-default.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 operator_note | PASS | `ExportRisk.operator_note` + `ExportRiskInputs.body_unavailable_winners`. Helper `export_risk_operator_note` -- Some only if fidelity Complete, level ReExportRecommended, sole elevating reasons keyed degrade +/- `poly_class_crc_discounted`, AND `body_unavailable_winners > 0`. Note includes keyed-rate fragment e.g. `keyed rate=0.049>0.02`. Assigned AFTER `finalize_unique_pst_classify` (also-eml combine). `emit_log("note: {note}")` before `--json` stdout. |
| DoD-2 Runbook §5 | PASS | poly vs BODY_UNAVAILABLE; do not re-export Permute for poly CRC. |
| DoD-3 Gate frozen | PASS | Threshold 0.02; `--fail-on-export-risk` still 65. No fourth enum. |
| DoD-4 Tests | PASS | `operator_note_after_also_eml_combine_not_pst_only` plus export_risk suite; workspace fmt/clippy/test. |
| DoD-5 Recorded | PASS | This file; registry Completed; CHANGELOG; ledger FEATURE committed on the product squash (`75d98b69-96b3-4514-889d-2c6a7aef8183`, merge `9082761`). |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass |
| CI (PR **#147**) | fmt, clippy, test (19m4s), audit, deny, chrome-ui, verify-parity **green**. Bugbot skipping (does not block). |
| Final cross-model gate | **PASS**, 0 findings (`conductor/0127-EmbeddedDepthOperatorHint/review.codex.round2.md`) |

## Reviewer rounds

1. Internal: engineering DoD met. Easy P3: helper labeled CRC-only as BODY_UNAVAILABLE -> fixed (`body_unavailable_winners == 0` -> None).
2. Codex round 1: **FAIL** -- 3 P2s, then fixed: (1) note assigned before also-eml combine, (2) unique-eml hint skipped on late Err, (3) note omitted numeric keyed rate.
3. Internal re-review: **PASS**, no >low.
4. Codex round 2 (`conductor/0127-EmbeddedDepthOperatorHint/review.codex.round2.md`): **PASS**, 0 findings. Final cross-model gate for Series U.

## HITL (owner)

CI uses unit fixtures, not INC*. Owner INC* re-smoke is optional (not a gate). Never commit INC* PSTs. Codesign is **D-0062-codesign**.

## Residual lows (deferred)

| ID | Item |
|---|---|
| (this track) | none above low |
| D-0127-* / D-0067 | see 0127 |
| 0131 HITL | owner `--prefer-folder-class` skipped -- see 0131 |
| Owner INC* re-smoke | optional, not a gate |

## Publish

- Branch: `track/0127-0132-series-u`
- PR: **#147** https://github.com/Ryan-AI-Studios/pst-dedupe/pull/147
- Merge SHA: `90827614db3b5149ecc8560132f3a0640b14eb9a` (short `9082761`)
- Commit: `track(0127-0132): unique-pst HITL residuals (depth hint, risk note, timings) (#147)`
- Ledger FEATURE tx `75d98b69-96b3-4514-889d-2c6a7aef8183` COMMITTED on the product squash
- Locks held: no BCC-default; do not raise `--max-embedded-depth` default (stays 3); identity depth 3; D-0067-embedded-depth stays open; no fourth ExportRisk enum; threshold 0.02; do not discount BODY_UNAVAILABLE; never fudge unaccounted_ms to 0; QC default stays sample; `--prefer-folder-class` stays opt-in; no `--progress-file`; JSON stays stdout
