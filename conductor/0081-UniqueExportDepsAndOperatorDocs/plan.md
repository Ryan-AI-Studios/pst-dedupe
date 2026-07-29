# 0081 — Unique Export Dep Pins & Operator Docs — Plan

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\plan.md`.
> Phased checklist; each phase maps to DoD items in `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.

> **Ledger:** open a transaction before starting —
> `ledgerful ledger start 0081-UniqueExportDepsAndOperatorDocs --category DOCS --message "<intent>"`
> (use `INFRA` for dep/deny-only sessions if preferred) — and commit it in the final phase.

---

## Phase 0 — Precondition / hygiene gate → DoD-15

- [ ] Confirm board: 0071–0080 Completed; workspace builds.
- [ ] Confirm accidental mangled-path directories are **absent** (already deleted 2026-07-29 if prep ran):
  - `C:\dev\Dedupe\C…devdedupeconductortrack011-pst-writer-eml-import`
  - `C:\dev\Dedupe\C…devdedupecratespst-writersrc`
- [ ] `ledgerful ledger status --compact`; start ledger tx for the implementation session.
- [ ] Re-read `spec.md` §2.5 locked rules and §2.7 decisions (Q1–Q7 locked — do not re-litigate).

## Phase 1 — Dependency audit + deny hygiene → DoD-1, DoD-2, DoD-8

- [ ] Run `cargo tree` depth-1 for export crates + `cargo tree -i` for duals (sha2/thiserror/rand/reqwest).
- [ ] Re-query crates.io maxes if >7 days after 2026-07-29 snapshot.
- [ ] Apply approved bumps: clap, serde_json, thiserror 2.x, camino, uuid (if tests green).
- [ ] **Do not** major eframe/reqwest/aes-gcm/rand/md-5 product pin unless security override (§2.5 rule 2).
- [ ] Record **rand KEEP + RUSTSEC-2026-0097** in draft audit notes.
- [ ] Prune dead `deny.toml` ignores (RUSTSEC-2026-0186, -0190, -0194, -0195); keep live rsa + ttf-parser.
- [ ] `cargo deny check` green.
- [ ] Draft audit table for `review.md` (finalize in Phase 6).

## Phase 2 — Basename path mode (ships) → DoD-9, DoD-12, DoD-16

- [ ] Implement `--ledger-path-mode full|basename` on unique-pst path (default `full`).
- [ ] Apply to **both** `export_messages.csv` and `export_attachments.csv` path columns.
- [ ] Keep `source_id` as join key.
- [ ] Unit/integration tests: full vs basename; basename non-empty when full had a path.
- [ ] `cargo test -p pst-dedup-cli` (and any shared engine crate) green.

## Phase 3 — Timing script → DoD-10

- [ ] Add `scripts/unique-pst-timing.ps1` (parameterized inputs/out/report-dir; no hardcoded client paths).
- [ ] PowerShell-native only (no bashisms).
- [ ] Optional `timing.json` sidecar; document usage in runbook (Phase 4).

## Phase 4 — Operator eDiscovery runbook → DoD-3, DoD-6, DoD-7, DoD-11, DoD-12, DoD-13, DoD-14

- [ ] Create `docs/unique-pst-ediscovery-runbook.md` with all §2.10 sections **0–11**.
- [ ] Collection/custody (§0): Purview preferred for M365; soft preference only.
- [ ] Outlook honesty: re-open current Microsoft Support docs same day; cite **access date** (DoD-11).
- [ ] Integrity table: numeric defaults (0.05 / 0.01 / 0.05 / 0.15 / 0.50 dual-rate).
- [ ] Exit table + **no blanket retry exit 5**.
- [ ] ScanPST copy-only + two-command count-diff (§2.11).
- [ ] Basename custody / Matter Archive mandate + “not full de-identification.”
- [ ] Disposition & secure purge (§8) — operator procedure, no product wipe claim.
- [ ] Optional redacted INC historical numbers (not SLA).
- [ ] Link timing script and `phase_timings`.

## Phase 5 — Doc links + deferred hygiene → DoD-4, DoD-5

- [ ] Link runbook from `README.md`.
- [ ] Expand `docs/operator-golden-path.md` Path B (3–10 lines + link).
- [ ] Top pointer in `docs/unique-pst-export.md`.
- [ ] Update `docs/deferred.md`:
  - D-0073-basename → closed (shipped 0081)
  - D-0077-repair-diff → closed (docs / 0081)
  - D-0078-retryable → residual code; runbook constraint satisfied

## Phase 6 — Full verification + finalize → DoD-16, DoD-17 (and residual DoDs)

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo deny check`
- [ ] `ledgerful verify` (or justified fallback)
- [ ] Manual docs checklist: all DoD-3..14 criteria satisfied.
- [ ] Write `review.md`: audit table final, evidence of bumps, runbook path, basename tests, deny prune delta, Outlook citation date, deferred closes, any residual.
- [ ] Update `../conductor.md`: 0081 status → **Completed**.
- [ ] Commit ledger transaction(s) in the execution repo.
- [ ] Notify: Series L unique-export hardening docs closed; no downstream track blocked (0081 is series closer unless product opens 0082+).

---

## Handoff notes

- **Outward-facing:** operator runbook becomes counsel-facing; treat language as defensible (preferred collection, not hard product gates).
- **Irreversible:** deleting accident dirs already done (empty only). Do not force-push or rewrite published release notes in this track.
- **Do not:** mid-RC majors without rule-2 security justification; invent new exit codes; implement secure-wipe; hard-block non-Purview PSTs; re-open Outlook COM.
- **Rollback:** dep bumps via lockfile revert; basename behind default-`full` so behavior is inert without flag.
- **PowerShell:** no `&&` / bash constructs in scripts or doc examples.
