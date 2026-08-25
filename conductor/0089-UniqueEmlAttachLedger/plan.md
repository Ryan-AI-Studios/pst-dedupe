# 0089 — Unique-EML Attach Ledger Parity — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-24):** `EmlAttachEvent` DTO in `eml_pack`; CLI drain + Mode A soft-skip; pack-root CSV; fail-closed ledger init — see `spec.md` §2.5.

> **Ledger:** `ledgerful ledger start crates/dedup-engine --category FEATURE --message "0089 unique-eml attach ledger (eml_pack events + CLI sink)"`
> (covers `dedup-engine` + `pst-dedup-cli`; do not start CLI-only)
> **TX:** `36f4223f-8c7f-4824-84ae-c8af743d81ca` (committed on DoD-6 finalize)

---

## Phase 0 — Design lock → DoD-1 (partial)

- [x] Diff unique-pst attach-ledger flags vs `UniqueEmlCliArgs`.
- [x] Inventory `EmlWriteError` / soft-fail sites in `eml_pack.rs`.
- [x] **Lock reason mapping table:** EML failure → 0073 `reason_code`; unmapped → generic documented code (never drop).
- [x] Confirm `--out/export_attachments.csv` (pack root). Confirm whether unique-eml already has `--report-dir`.
- [x] Confirm Mode A: `soft_skip_attach_records` + `mark_promoted_winner` must be wired (hard, not optional).

## Phase 1 — Engine DTO + CLI sink → DoD-1, DoD-2, DoD-3

- [x] Add `EmlAttachEvent` + `EmlWriteResult.attachment_events` in `dedup-engine`.
- [x] Populate events at every current `attach_parts_failed` increment site.
- [x] CLI: args + `AttachLedgerSink`; map events → `AttachLedgerRow` (reuse header constant).
- [x] Drain `resolved.soft_skip_attach_records`; call `mark_promoted_winner`.
- [x] Fail closed if sink/CSV init fails in `full`.
- [x] No production `unwrap`/`expect`; no engine→CLI dependency.

## Phase 2 — Tests → DoD-2, DoD-4

- [x] Integration: soft-fail attach → CSV present + **header equals** `EXPORT_ATTACHMENTS_CSV_HEADER`.
- [x] Mode A: promoted winner + loser attach rows.
- [x] Row-cap truncated marker.
- [x] Ledger `off` still classifies exit 64 from counters.
- [x] Ledger init fail → non-success / fail-closed (not silent continue).

## Phase 3 — Docs + finalize → DoD-5, DoD-6

- [x] CLI help + operator note (CSV path, path-mode).
- [x] Close `D-0073-eml` in `docs/deferred.md`.
- [x] Write `review.md`; mark conductor **Completed**; commit ledger TX.

---

## Handoff notes

- DTO in engine, sink in CLI — never pass `AttachLedgerSink` into `eml_pack`.
- Do not change eml_pack MIME layout.
- Can run in parallel with 0088.
