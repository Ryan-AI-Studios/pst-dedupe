# 0000 — <Track Title> — Plan

> Template. Phased checklist; map each phase to the DoD items in `spec.md` §7. Execute in the
> execution repo named in `spec.md`. Mark items `- [x]` as completed.

> **Ledger:** open a transaction before starting —
> `ledgerful ledger start <slug> --category <CATEGORY> --message "<intent>"` — and commit it in the
> final phase.

---

## Phase 0 — Precondition / acceptance gate → DoD-<n>
- [ ] <…>

## Phase 1 — Implementation → DoD-<n>
- [ ] <…>

## Phase N — Finalize → DoD-<last>
- [ ] Write `review.md` in this track dir: results, evidence, and any explicitly-deferred items.
- [ ] Update `../conductor.md`: set this track's status to **Completed**.
- [ ] Commit the ledger transaction in the execution repo.
- [ ] Notify any downstream tracks this unblocks.

---

## Handoff notes
- <Which phases are outward-facing/irreversible; rollback steps; anything an implementer must not do.>
- Single-exe / no-daemon constraint must remain true unless the track explicitly changes product policy.
