# 0038 — Case overview dashboard — Plan

Phased checklist. Map phases to DoD items in `spec.md` §7. Execute in `C:\dev\dedupe`.

> **Ledger:** `ledgerful ledger start 0038-caseoverviewdashboard --category FEATURE --message "Case overview dashboard"` — commit in Finalize.

---

## Phase 0 — Preconditions → DoD-3 baseline
- [ ] Confirm dependency tracks complete or document allowed parallel work: **0019,0025**
- [ ] Read `C:\dev\Dedupe-plan.md` sections for Series E
- [ ] `cargo check --workspace` (or narrower package set) green
- [ ] Identify fixtures (synthetic Purview layout / sample PST under `fixtures/`)

## Phase 1 — Design / schema / API sketch → DoD-1 prep
- [ ] Document public types, table changes, or CLI flags this track introduces
- [ ] List files/crates expected to change
- [ ] Note security considerations (zip paths, secrets, audit)

## Phase 2 — Implementation → DoD-1, DoD-2, DoD-4
- [ ] Implement capability
- [ ] Add/adjust tests
- [ ] Wire audit events where matter state changes
- [ ] Keep single-exe / no-daemon invariant unless this track's product decision says otherwise

## Phase 3 — Verification → DoD-2, DoD-3
- [ ] Run track tests + workspace gate commands from `spec.md` §8
- [ ] Manual smoke on Windows if UI involved
- [ ] Capture command outputs / counts for `review.md`

## Phase 4 — Finalize → DoD-5, DoD-6
- [ ] Write `review.md` (results, evidence, deferred items)
- [ ] Update `../conductor.md` status to **Completed**
- [ ] Update `../sequencing.md` if spine changes
- [ ] Commit ledger transaction
- [ ] Notify downstream tracks this unblocks

---

## Handoff notes

- Downstream tracks depend on this completing: see `../sequencing.md`.
- Do not rewrite source Purview/PST evidence files.
- Prefer extending existing crates over inventing parallel parsers.
