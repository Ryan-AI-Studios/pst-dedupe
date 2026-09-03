# 0130 — QcSampleCost — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Ledger (execute):** `ledgerful ledger start 0130-qc-sample-cost --category FEATURE --message "QC sample cost honesty; default stays sample"`
>
> **Fold-in 2026-09-03:** opencode-M1/m1/m2/O1 + AGY-130-01..02.

## Phase 0 → DoD-4

- [ ] Re-read `--qc-level` default `sample` and both QC stderr lines. Confirm `qc_ms` is already on `PhaseTimings`.
- [ ] Do **not** change the default. Do **not** skip source-differential. Do **not** plan a sample-speed rewrite.

## Phase 1 → DoD-1, DoD-2, DoD-3

- [ ] stderr-first: add `qc_ms={}` to **both** `qc ok:` and `qc hard findings:` via `emit_log`.
- [ ] Test hook: capture QC-complete stderr/on_log and assert `qc_ms=` (do not rely on `unique_pst_qc_0080` for the format).
- [ ] Runbook timing table (sample / structure / off / full). Do not claim `qc-pst` differs.
- [ ] `unique-pst-export.md` `qc_ms` row (coordinate with 0129 if same release).
- [ ] Confirm 0080 tests still pass; clap default still `sample`.

## Phase 2 → DoD-4, DoD-5

- [ ] `cargo test -p pst-dedup-cli --test unique_pst_qc_0080` + stderr `qc_ms=` hook
- [ ] fmt / clippy / CHANGELOG / `review.md` / registry Completed / ledger commit FEATURE
