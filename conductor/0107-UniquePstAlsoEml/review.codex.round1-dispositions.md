# Codex audit round-1 dispositions — 0107 UniquePstAlsoEml

Source: `review.codex.md` (FAIL @ `069e6d7`). Orchestrator-validated findings only.

| Finding | Disposition | What changed |
|---|---|---|
| [P1] Standalone unique-eml `--fail-on-export-risk` silently ignored | **Validated / Fixed** | Helper takes `risk_gate` + `export_risk`. `run_unique_eml` parses CLI gate (pre-0107 behavior). also-eml keeps `RiskGate::Off` + `PreflightRecommendation::Ok`. Regression: `unique_eml_fail_on_export_risk_ok_exits_65`. |
| [P1] Helper hard failures omit `{also-eml}/summary.json` | **Validated / Fixed** | Wrapper calls `write_eml_hard_fail_summary` on helper `Err`. unique-pst also synthesizes if missing. Test: `helper_hard_fail_writes_summary_json`. PST not quarantined. |
| [P2] EML cancel quarantine leaves stale artifact metadata | **Validated / Fixed** | `quarantine_also_eml_dir` returns dest path; `rewrite_quarantined_eml_summary` sets `artifact_state=partial_quarantined` + updated `out`/`summary_path`. Test: `rewrite_quarantined_summary_sets_partial_quarantined`. |
| [P2] Combined co-export failure lacks production-path tests | **Validated / Fixed (partial)** | Added hard-fail summary + quarantine rewrite tests; kept Generic>Partial unit test. Full cancel-during-PST / cancel-during-EML integration remains residual (injectable unit/lib coverage preferred). |
| [P2] Standalone unique-eml final summary rewrite ignores write failure | **Validated / Fixed** | Stitch rewrite propagates serialize/write failure as report hard-fail (`exit=1` / `AlreadyEmitted`); no silent `let _ = fs::write`. |
| [P1] DoD-4 review.md / Completed | **Wontfix (orchestrator-owned)** | Registry stays In progress; no final track `review.md` DoD matrix this pass. |

Ledger FIX: `d29261d6-1dc5-4282-93d1-60e699700e6b`
