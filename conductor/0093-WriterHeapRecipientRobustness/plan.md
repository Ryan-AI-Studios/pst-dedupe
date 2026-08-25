# 0093 — Writer Heap + Recipient Robustness — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-25):** Strategy **B** locked; 2048 is a documented single-page deviation;
> cumulative/adaptive heap budget; budget-aware recipient cap (To>Cc>Bcc); QC KnownGap via writer
> event (not `out==48`); `message_class` on the helper; spawn `D-0093-attachment-tc-page` —
> see `spec.md` §2.7.

> **Ledger:** `ledgerful ledger start crates/pst-writer --category BUGFIX --message "0093 heap diversion + recipient TC honesty"`
> Active TX: `e34ed7b5-6564-44da-b58a-e8bdc4f157e1` (do not start another; orchestrator commits).

---

## Phase 0 — Design lock → DoD-3 (partial), DoD-4 hygiene

- [x] Confirm Strategy **B** (spec §2.6). Do **not** reopen A vs B.
- [x] Lock `MAX_HEAP_VALUE_SIZE` = 2048 as a **documented HeapBuilder deviation** (spec §2.4). Record multi-block HN / 3580 restore research on `D-0093-recipient-tc-multipage` — do not implement.
- [x] Pick QC wiring: dedicated QC branch that reads the writer truncate event → `KnownGap`. Do **not** mid-v1 rewrite `recipient_table` Preserved.
- [x] Inventory uncommitted `production.rs` / `lib.rs` (`try_alloc` diagnostics, not fixture `build_pc_v2`). Rebase on `main` if needed.
- [x] **Hygiene:** untracked `crates/pst-reader/examples/probe_out.rs` — deleted. `cargo fmt --all --check` must not fail on it.
- [x] Confirm residuals to keep/spawn: close `D-0068-01` at land; keep `D-0093-recipient-tc-multipage`; spawn `D-0093-attachment-tc-page`.

## Phase 1 — String diversion → DoD-1, DoD-3

- [x] Land `push_string_prop` diversion for MID / subject / sender / Display* / **`message_class`**.
- [x] Lower `MAX_HEAP_VALUE_SIZE` with a comment that cites **single-page heap budget**, not “inherent to MS-PST.” Fix overstated module-doc language if still present.
- [x] **Cumulative / adaptive budget:** MessageSize probe heap — escalate largest remaining inline helper strings and re-probe (`spec.md` §2.5).
- [x] Fidelity/unit test: **multiple** 1.5–2 KiB helper strings (subject + sender + DisplayTo + DisplayCc at minimum) write and re-read. Single >2 KiB DisplayTo is not enough.
- [x] Close `D-0068-01`.

## Phase 2 — Recipient TC Strategy B → DoD-2

- [x] Replace fixed `&rows[..48]` with **budget-aware** stop (catch-and-retry). 48 may be a starting maximum; event reports **actual** kept.
- [x] Before capping: keep `MAPI_TO` then `MAPI_CC` then `MAPI_BCC` (stable within class). Display* on the message PC stay full.
- [x] WARN → structured counters (`recipient_tc_truncated_messages`, `recipient_rows_truncated`) + `RECIPIENT_TC_TRUNCATED` event (source/kept + per-class To/Cc/Bcc kept/dropped). Reuse `attachment_fidelity_events` capped-Vec + exact-counter shape.
- [x] QC: truncate **with** matching writer event → `FindingClass::KnownGap`. Predicate is the event, **not** `out.len()==48 && subset`. Unexplained mismatch without an event stays `Defect`.
- [x] Add `max_by_key(display_to.len())` to `select_sample_indices`.
- [x] Synthetic fixture: ≥136 recipient rows; assert write completes; QC `known_gap` not `defect`; Display* round-trip full.

## Phase 3 — Finalize → DoD-3, DoD-4, DoD-5

- [x] Docs: `docs/pst-writer-fidelity-v1.md` (2048 deviation, Strategy B, budget-aware cap) + unique-pst export note.
- [x] Deferred: `D-0068-01` closed; `D-0093-recipient-tc-multipage` notes include §2.4 MS-PST sketch; `D-0093-attachment-tc-page` present.
- [x] `review.md`; conductor **Completed**; ledger commit.
- [ ] Operator re-smoke optional on INC0102784 (0 heap overflow; recipient policy per DoD-2).

---

## Handoff notes

- Uncommitted local fix already proves diversion unblocks operator write — do not regress.
- Do not silently clip Display* to “fix” the heap.
- Do not ship a 48-row slice as if it were a byte bound.
- Embedded nested export is **0094**, not this track.
- Attachment-table TC overflow is **D-0093-attachment-tc-page**, not a silent out-of-scope.
