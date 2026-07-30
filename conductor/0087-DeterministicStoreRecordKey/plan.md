# 0087 — Deterministic Store Record Key — Plan

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\plan.md`.
> Phased checklist; each phase maps to DoD items in `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Research (2026-07-29):** D-0079-deterministic-key is the board-first Series M residual after
> 0086; sha2 0.11.0 KEEP; no new deps; sovereign hosts research handoff only (§2.9) — not folded.
>
> **Review fold-in (2026-07-29):** length-prefixed preimage fields (no null-term variable framing);
> volume-layout coupling honesty + preferred job-global `store_key_material`; DoD-3 structural
> fallback (0079 oracle) if volume-file hash drifts; Phase 0 HashMap/HashSet write-order audit;
> **cross-process** CLI tests for DoD-2/DoD-3; optional 0086 seed synergy docs-only — see
> `spec.md` §2.13.
>
> **Ledger:** FEATURE tx already started by implementer session —
> `312af2d8-dab3-4829-8f33-79be6a16c7c1` — orchestrator commits.

---

## Phase 0 — Precondition / design lock · DoD-8 (partial)

- [x] Confirm board: 0082–**0086 Completed**; **D-0079-deterministic-key open** in `docs/deferred.md`.
- [x] Re-read `generate_store_record_key` + call site (~L1639) + ProviderUID builders.
- [x] **Phase 0 non-determinism inventory (final PST bytes only):**
  - Grep `pst-writer` + unique-pst materialize/write path for `SystemTime::now`, `process::id`, `rand`, `Uuid::new_v4` / `uuid::`.
  - **Also audit:** `HashMap` / `HashSet` / `BTreeMap` where `.iter()` / `.values()` / `.keys()` determines write order, NID assignment, or folder order (Rust `RandomState` is per-thread; cross-process re-export is the risk). Confirm live path: unique-pst `prepared_by_locus` is lookup-only; keepset rebuild uses Vec order (§2.2).
  - Classify each hit: final bytes vs staging/temp/test-only.
  - Record in working notes; if extra final-byte entropy exists, decide fix-in-scope vs **D-0087-*** + DoD-3 structural fallback.
- [x] **Lock preimage** exactly as `spec.md` §2.6:
  - Domain sep + algo v1 + volume_index + count + content_fingerprint
  - **Length-prefixed** variable fields only (`len_u32_le || utf8`) — **no** `field || \0` framing
  - Job-global `store_key_material` preferred when unique-pst can supply it
- [x] **Lock path independence:** dest path **not** in preimage.
- [x] **Lock volume-layout honesty:** changing `--max-volume-bytes` breaks per-volume key/digest repro — document in runbook (not a bug).
- [x] **Lock DoD-3:** byte-identity best-effort; structural oracle fallback is a valid pass.
- [x] **Lock default deterministic**; ephemeral only if CLI flag is cheap — otherwise opts-only / omit flag.
- [x] Confirm multi-volume path: where unique-pst chooses volume index and calls streaming write.
- [x] Confirm message fields available for derived fingerprint (MID, subject, submit time, folder path).
- [x] Confirm cheap job-global seed source (ordered keep-set winner loci digest or keep_set file digest).
- [x] Re-query crates.io if >7 days after 2026-07-29; expect sha2 **KEEP 0.11.0**.
- [x] `ledgerful ledger status --compact`; start FEATURE ledger tx.
- [x] Re-read `spec.md` §2.5–§2.8 + §2.13 fold-in — **do not** fold D-0073-eml / sovereign / named-prop write / 0086 residuals as code scope.

---

## Phase 1 — Pure key derivation · DoD-1, DoD-4, DoD-5 (unit)

- [x] Implement pure `derive_store_record_key(...)` (or module-private with `#[cfg(test)]` re-export).
- [x] Implement length-prefixed volume_local_fingerprint (§2.6.2).
- [x] Implement job-material + volume re-bind path (§2.6.1).
- [x] Implement all-zero guard.
- [x] Unit tests: golden bytes; different content; different volume_index; seed override;
      **boundary case** that would collide under null-terminated framing but not under length-prefix.
- [x] No production `unwrap`/`expect`.

---

## Phase 2 — Wire production writer · DoD-1, DoD-4

- [x] Extend `WritePstOpts` (or write entrypoints) with:
  - `volume_index: u32` (default 0)
  - `store_key_material: Option<[u8; 32]>` (default None → volume-local derive only)
  - optional `store_record_key_mode` if ephemeral ships
- [x] Replace `generate_store_record_key` call path to use pure derivation over **messages written**.
- [x] Preserve ProviderUID == RecordKey invariant in all EntryIDs.
- [x] Redesign `store_record_key_differs_across_separate_writes` (different content or volume_index, **not** wall clock).
- [x] Keep self-consistency test green.

---

## Phase 3 — unique-pst plumbing + key/digest proof · DoD-2, DoD-3, DoD-5

- [x] Thread `volume_index` from multi-volume unique-pst writer into opts.
- [x] When cheap: compute **job-global** fingerprint (ordered winners / keep-set) → `store_key_material`.
- [x] Optional: summary JSON `store_record_key_mode`.
- [x] **DoD-2 in-process:** same winners, two dest paths → same RecordKey (writer unit/integration).
- [x] **DoD-2 cross-process (required):** spawn built CLI twice via `std::process::Command` (or equivalent), identical inputs, different dests → same RecordKey from both PSTs.  
  - Do **not** rely solely on two in-process calls for the CLI-level claim.
- [x] **DoD-3 attempt:** same pair → compare volume `sha256_hex`.
  - If match → path A (byte-identity observed).
  - If mismatch → run **0079 structural equivalence oracle**; if equivalent, document path B + residual/runbook honesty; **pass DoD-3**. If not equivalent, investigate before closing.
- [x] CLI help / flag only if ephemeral mode ships. (opts-only; no CLI flag)

---

## Phase 4 — Docs + deferred · DoD-6

- [x] `docs/unique-pst-export.md`: CoC / re-run reproducibility; **volume-layout coupling**; RecordKey vs volume-hash honesty; optional 0086 `store_key_material` synergy; flag table if any.
- [x] `docs/unique-pst-ediscovery-runbook.md`: custody meaning under default deterministic key; layout coupling; B-tree/layout best-effort sentence.
- [x] `docs/deferred.md`: **close D-0079-deterministic-key**; leave D-0085 / D-0073-eml / D-0084 / D-0086-* open; optional D-0087-* if residual layout entropy.
- [x] CHANGELOG `[Unreleased]`.
- [x] Board next-candidate line: D-0073-eml, D-0085-sovereign (research-unlocked), D-0084 named-prop write, D-0086-*.

---

## Phase 5 — Gates + finalize · DoD-7, DoD-8

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test -p pst-writer`
- [x] `cargo test --workspace` (or justified narrow + full before commit)
- [x] `ledgerful verify` (or report fallback)
- [x] Write `review.md`: preimage formula (length-prefix), Phase 0 inventory (incl. HashMap), fold-in §2.13, DoD-3 path A or B, test evidence (cross-process), residuals, dep KEEP.
- [x] Update `../conductor.md` + `ROADMAP.md`: 0087 → **Completed** (on ship).
- [ ] Commit FEATURE ledger tx. **Leave for orchestrator.**
- [ ] Notify: D-0079-deterministic-key closed; next Series M candidates unchanged except order note for D-0085 research.

---

## Handoff notes

- **Hard product guarantee:** deterministic RecordKey / ProviderUID under default mode (logical store identity).
- **Soft product claim:** full volume-file `sha256_hex` match — best-effort; structural oracle is valid DoD-3 exit.
- **Irreversible product effect:** re-exports no longer get random store keys; document CoC meaning.
- Do not claim sovereign-cloud body URL coverage (D-0085 still open).
- Do not touch temp-staging entropy (not final bytes).
- Single-exe / offline invariant unchanged.
- Cross-process CLI tests are load-bearing for HashMap-order regressions — do not delete them to “save CI time” without a replacement that is also multi-process.
