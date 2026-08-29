# Codex audit round-4 dispositions — 0107 UniquePstAlsoEml

Source: Codex FAIL @ `67c2643` (single P2). Orchestrator-validated.

| Finding | Disposition | What changed |
|---|---|---|
| [P2] Also-eml hard-fail fallback loses outcome provenance | **Validated / Fixed** | `write_eml_hard_fail_summary` takes real `scan_ok`; recovers counts from existing summary/manifest/on-disk `.eml`; does not clobber usable summaries. unique-pst non-cancel `Err` sets `REPORT_WRITE_FAILED` reasons and fills `also_eml_*` counters via `also_eml_recovered_counts`. Tests: `hard_fail_summary_uses_real_scan_ok_false`, `hard_fail_summary_does_not_clobber_usable_summary`. |

DoD-4 Completed / final `review.md` — still orchestrator-owned.

Ledger FIX: `152503ec-1eb7-4900-aae0-97cb3a5f173f`
