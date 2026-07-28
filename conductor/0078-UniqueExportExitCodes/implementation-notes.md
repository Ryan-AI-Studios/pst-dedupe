# 0078 Implementation Notes

## Layout

| Piece | Location |
|---|---|
| `classify_export` / fidelity / artifact_state | `crates/pst-dedup-cli/src/export_outcome.rs` |
| CliExit 64/65/130 | `crates/pst-dedup-cli/src/error.rs` |
| unique-pst wiring + quarantine | `crates/pst-dedup-cli/src/unique_pst_cmd.rs` |
| JSON fields | `UniqueExportSummary` in `unique_export_report.rs` |
| Plumbing `run -> Result<CliExit>` | `main.rs` |
| Baseline (pre-change) | `baseline.md` |

## Handoff to 0081 (operator runbook)

- Document exit matrix with **severity before numeric** ordering.
- **Anti-recommendation:** do **not** advise blanket “retry exit 5”. `CliExit::MatterIo` covers `Io`/`Sqlite` (plausibly transient) **and** `AuditChainBroken`, `SchemaVersionMismatch`, `WrongPassphrase`, `DatabaseMissing` — retrying a broken audit chain delays escalation of the worst integrity failure. Retryability is future `retryable: bool` JSON (D-0078-retryable), not the integer.
- Recommend `--fail-on-export-risk not_export_ready` for legal-hold pipelines (tool default remains **off** for refinement rule 4).
- `.partial` retention / cleanup is an operator concern.

## Handoff to 0080 (QC)

- Branch QC scripts on 0 / 64 / 65 / 130 / 1; treat 64 as “review attach ledger, artifact may ship with disclosure”.

## Residuals (not closed)

- D-0073-eml — full unique-eml ledger CSV (narrow counters only in 0078)
- D-0045-02 — cross-process cancel
- D-0078-retryable, D-0078-gui
