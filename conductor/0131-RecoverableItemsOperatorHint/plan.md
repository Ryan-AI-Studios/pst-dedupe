# 0131 — RecoverableItemsOperatorHint — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Ledger (execute):** `ledgerful ledger start 0131-ri-hint --category DOCS --message "Runbook golden-flow for opt-in --prefer-folder-class"`
>
> **Fold-in 2026-09-03:** opencode-m1/m2 + AGY-131-01 (partial / decline as stated) / AGY-131-02.

## Phase 0 → DoD-4

- [ ] Re-read `recoverable_items_hint`, unique-pst `emit_log` call, unique-eml `eprintln` when `!json`, runbook §3 RI row. Confirm folder-class default is off.
- [ ] Do **not** default-on. Do **not** change source-rank. Do **not** treat RI as a defect. Do **not** insert a newline in the keepset string. PowerShell wrap is **0132**. Do **not** add unique-eml `--json` hint.

## Phase 1 → DoD-1, DoD-2, DoD-3

- [ ] Expand runbook golden-flow (optional flag; Sent/live over RI; INC* ~20% expected without flag; unique-pst vs unique-eml hint channels).
- [ ] Optional cheap unit: hint text contains `--prefer-folder-class`. Ranking tests unchanged.
- [ ] Optional owner HITL with flag — winner-from-RI delta only; skip is allowed if recorded.

## Phase 2 → DoD-4, DoD-5

- [ ] `cargo test -p dedup-engine recoverable` (no-regression)
- [ ] CHANGELOG if user-facing / `review.md` / registry Completed / ledger commit **DOCS**
