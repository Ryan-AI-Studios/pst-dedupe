# 0132 — CliProgressPowershell — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Ledger (execute):** `ledgerful ledger start 0132-ps-progress --category DOCS --message "Runbook PowerShell stderr capture; JSON stays stdout"`
>
> **Fold-in 2026-09-03:** opencode-m1/m2 + AGY-132-01..02.

## Phase 0 → DoD-4

- [ ] Re-read progress-on-stderr and `scripts/unique-pst-timing.ps1` `ProcessStartInfo`. Confirm `--json` is stdout.
- [ ] Do **not** move progress onto stdout. Do **not** add `--progress-file`.

## Phase 1 → DoD-1, DoD-2, DoD-3

- [ ] Runbook section with exact `cmd /c "… 2> progress.log"` and `Start-Process` (or timing-script) snippets. No `&&`. Explain NativeCommandError wrapping.
- [ ] Pointer from day-1 `scan --json | Set-Content` to that section (recipe applies to any `pst-dedup` stderr).
- [ ] Timing-script header: why `RedirectStandardError = $false`; pointer to runbook.
- [ ] Code change only if docs cannot close; then `--json` fixture.

## Phase 2 → DoD-4, DoD-5

- [ ] Docs-only: no clippy required. If Rust: fmt/clippy + json stdout test.
- [ ] CHANGELOG if user-facing / `review.md` / registry Completed / ledger commit **DOCS**
