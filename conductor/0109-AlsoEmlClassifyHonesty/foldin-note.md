# 0109 fold-in — 2026-08-29

Sources (not edited): `opencode-review.md`, `agy-review.md`.

Status stays **Ready — not started**. No product crates.

## Folded

- **opencode-m1:** `finalize_unique_pst_classify` is fidelity worse-of only `(pst, Option<ExportFidelity>)`. Exit/reason merge stays at live combined-exit block. `ok` at call site.
- **opencode-m2:** Recovery test trigger = seed `summary.json` **file** + `manifest.json` **directory** + `cancel=true` (same Err as `helper_hard_fail_writes_summary_json`).
- **opencode-o1:** DoD-3 / §3.4 — attach/embedded from JSON only.
- **opencode-o2:** §3.6 — allow-partial summaries emit `error.code=partial_fidelity`.
- **opencode-o3:** §2.2 row 3 parenthetical (exit 64 was never `ok=true`).
- **agy** false-pass / pre-seed: helper tests inject `ExportFidelity::Partial`; recovery seeds non-zero JSON.

## Already covered (agy Majors / F-0109-1..4)

The three Bugbot sites plus `artifact_state` pre-merge lock were already DoD-1–3 / §2.3.

## Declined

- Helper taking EML `exit`/`reasons` (opencode-m1 alternative) — double-merge risk.
- Recovering `attach_parts_written` on cancel-Ok — not an `also_eml_*` summary key.
- agy ident `CliExit::RiskGate` — live name is `ExportRiskBlocked`.
- New deferred rows — none.

## Not done

Harness `*-review.md` files were not modified. Track not implemented.
