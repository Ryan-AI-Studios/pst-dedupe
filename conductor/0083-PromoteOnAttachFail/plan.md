# 0083 — Promote on Attach Fail (Mode A) — Plan

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\plan.md`.
> Phased checklist; each phase maps to DoD items in `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.

> **Ledger:** open a transaction before starting —
> `ledgerful ledger start 0083-PromoteOnAttachFail --category FEATURE --message "<intent>"`
> — and commit it in the final phase.

> **Revised 2026-07-29:** dual-AI review fold-in (Sedona cross-custodian, dup_sources, QC×Mode A, Mode C fallback decided_by, tier/cloud honesty).

---

## Phase 0 — Precondition / ground truth → DoD-13

- [x] Confirm board: 0073–0082 **Completed**; workspace builds.
- [x] Re-read `finalize_with_materialize` hard-fail promote loop; decision CSV; **`duplicate_sources` / `duplicate_source_count`** construction for Unique rows.
- [x] Confirm default grouping unites across sources; `--dedupe-scope per-source` isolates (`keepset.rs`).
- [x] Inventory attach-incomplete signals on `CanonicalMessage` / attach DTOs (`stream_available`, soft fidelity, 0074 probe fields).
- [x] Trace 0080 QC source-lookup path: must use **final** export winner locus / `source_id` from decisions or export_messages (not a cached pre-promote pick).
- [x] Grep for any partial `--promote-on-attach-fail` stub — must not exist twice.
- [x] Re-query crates.io if >7 days after 2026-07-29; expect **no bumps**.
- [x] `ledgerful ledger status --compact`; start FEATURE ledger tx.
- [x] Re-read `spec.md` §2.5 rules and §2.9 Q1–Q13 — do not re-litigate; **Mode B forbidden**; **no cloud detector invent**; **no least-incomplete ladder**.

## Phase 1 — Incomplete predicate → DoD-2

- [x] Implement centralized `is_attach_incomplete` (name per crate style) in `dedup-engine`.
- [x] Cover rule 5 positives: `!stream_available`, pre-bound fail-severity attach outcomes.
- [x] Cover rule 5 negatives: body-only soft flags, `parents_only` omit, zero-byte success.
- [x] Code comment: cloud/modern attach not detectable without named props (D-0080-cloud-attachments).
- [x] Unit tests table-driven.
- [x] `cargo test -p dedup-engine` green for predicate module.

## Phase 2 — Mode A loop in materialize finalizer → DoD-3, DoD-4, DoD-5, DoD-6, DoD-10

- [x] Thread `promote_on_attach_fail: bool` into materialize finalization context (options struct — not a global).
- [x] Extend peer walk per spec §2.7.2:
  - Hard fail → continue (existing).
  - Soft incomplete + flag + more peers → skip candidate, continue.
  - Soft incomplete + flag + no more peers → accept Mode C fallback; **`decided_by=mode_c_fallback_all_peers_incomplete`**.
  - Complete → accept; if attempt > 0 → **`decided_by=promoted_after_attach_incomplete`**.
- [x] Set `promoted_from_failure` when accepted peer is not first attempt (including Mode C fallback after skips).
- [x] Preserve hard path string **`promoted_after_materialize_fail`**.
- [x] Stats: `promoted_after_attach_incomplete_count`, `mode_c_fallback_all_peers_incomplete_count`.
- [x] **dup_sources invariant:** after Mode A accept, Unique-row `duplicate_sources` still lists other group members (incl. skipped incomplete). Add unit test with multi-source group.
- [x] Tests:
  - Flag on: incomplete peer0 + complete peer1 → peer1 wins; `promoted_after_attach_incomplete`.
  - Flag off: incomplete peer0 accepted (Mode C), no promote.
  - All incomplete: highest-ranked materializable exported; `mode_c_fallback_all_peers_incomplete`; not group_dropped.
  - Hard fail still promotes (existing test green).
  - **No** least-incomplete re-rank (if fixture has less-incomplete lower peer, still pick highest-ranked materializable on fallback).
- [x] `cargo test -p dedup-engine` green.

## Phase 3 — CLI, summary, ledger, QC → DoD-1, DoD-7, DoD-8, DoD-11

- [x] Add `--promote-on-attach-fail` to unique-pst CLI + shared args (GUI pass-through default false).
- [x] Help text: default off; Mode A pre-write; Mode B not supported; **cross-custodian de-duplication** under global scope → see runbook.
- [x] Wire flag into unique-pst materialize/export path.
- [x] Summary JSON: `promote_on_attach_fail`, promote count, Mode C fallback count.
- [x] Attach ledger: `winner_promoted` / peer locus when applicable.
- [x] Integration tests under `pst-dedup-cli` (synthetic fixtures only).
- [x] Exit honesty: complete promote → no attach fails for that family when peer fully complete; incomplete fallback still partial.
- [x] **DoD-11:** Mode A promote fixture + QC sample (or in-process equivalent) → assert **no** spurious `unexplained_loss` from wrong source; findings clean for the promoted family (or only expected known_gap classes already in contract).
- [x] `cargo test -p pst-dedup-cli` green (targeted Mode A + lib; full suite for orchestrator gate).

## Phase 4 — unique-eml optional thread → (supports DoD-12 honesty)

- [x] If unique-eml uses the same finalizer: thread the flag; note in docs.
- [x] If not: document that Mode A is unique-pst primary; D-0073-eml remains residual for full ledger CSV.
- [x] Do **not** block track completion on full eml ledger parity.

## Phase 5 — Docs + deferred → DoD-9, DoD-12

- [x] Update `docs/unique-pst-export.md`: Modes A/B/C; flag; `winner_promoted`; three `decided_by` strings; identity-tier fracture note (D-0076-attach-content); Mode B declined.
- [x] Update `docs/unique-pst-ediscovery-runbook.md`:
  - When to enable; recommend `--deep-attach-preflight`
  - **Sedona “cross-custodian de-duplication”** naming under default global scope
  - Disclosure: use `duplicate_sources` / decisions for suppressed custodians
  - `--dedupe-scope per-source` confines Mode A
  - Filter `mode_c_fallback_all_peers_incomplete` for families Mode A could not complete
  - Cloud/modern attach honesty ceiling (cannot detect without named props)
- [x] Update `docs/deferred.md`: **D-0073-promote → closed / 0083**; Mode B declined; D-0073-eml disposition; point cloud / attach-content residuals.
- [x] CHANGELOG `[Unreleased]` Tier-1 entry (no version bump).

## Phase 6 — Full verification + finalize → DoD-14, DoD-15

- [x] `cargo fmt --all --check` (implementer + orchestrator)
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `cargo deny check`
- [x] `ledgerful verify` (or justified fallback + exact command)
- [x] Write `review.md`: Mode A evidence, three decided_by strings, dup_sources test, QC×Mode A evidence, deferred closes, dual-AI fold-in, Mode B/cloud/least-incomplete declines.
- [x] Update `../conductor.md`: 0083 → **Completed**; Series M next-candidate line updated.
- [x] Commit ledger transaction.
- [x] Notify: D-0073-promote closed; next Series M candidates (named props / cloud attach, D-0076-attach-content, deterministic key, D-0073-eml full).

---

## Handoff notes

- **Default off** is load-bearing — do not flip default mid-track without product call.
- **Mode B is forbidden** — residual with reason if re-raised.
- **No least-incomplete ladder** — rule 6; Mode C fallback is highest-ranked materializable only.
- **No invent cloud-attach incomplete** without named-prop reader work.
- Prefer extending `finalize_with_materialize` over a parallel promote engine.
- QC must key the **final** winner after Mode A — assert it; do not assume.
- Production forbids `.unwrap()` / `.expect()`.
- Rollback: feature flag off restores Mode C; leave flag wired but default false if blocked mid-ship.
