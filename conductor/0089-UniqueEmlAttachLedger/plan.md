# 0089 — Unique-EML Attach Ledger Parity — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-24):** `EmlAttachEvent` DTO in `eml_pack`; CLI drain + Mode A soft-skip; pack-root CSV; fail-closed ledger init — see `spec.md` §2.5.

> **Ledger:** `ledgerful ledger start crates/dedup-engine --category FEATURE --message "0089 unique-eml attach ledger (eml_pack events + CLI sink)"`
> (covers `dedup-engine` + `pst-dedup-cli`; do not start CLI-only)

---

## Phase 0 — Design lock → DoD-1 (partial)

- [ ] Diff unique-pst attach-ledger flags vs `UniqueEmlCliArgs`.
- [ ] Inventory `EmlWriteError` / soft-fail sites in `eml_pack.rs`.
- [ ] **Lock reason mapping table:** EML failure → 0073 `reason_code`; unmapped → generic documented code (never drop).
- [ ] Confirm `--out/export_attachments.csv` (pack root). Confirm whether unique-eml already has `--report-dir`.
- [ ] Confirm Mode A: `soft_skip_attach_records` + `mark_promoted_winner` must be wired (hard, not optional).

## Phase 1 — Engine DTO + CLI sink → DoD-1, DoD-2, DoD-3

- [ ] Add `EmlAttachEvent` + `EmlWriteResult.attachment_events` in `dedup-engine`.
- [ ] Populate events at every current `attach_parts_failed` increment site.
- [ ] CLI: args + `AttachLedgerSink`; map events → `AttachLedgerRow` (reuse header constant).
- [ ] Drain `resolved.soft_skip_attach_records`; call `mark_promoted_winner`.
- [ ] Fail closed if sink/CSV init fails in `full`.
- [ ] No production `unwrap`/`expect`; no engine→CLI dependency.

## Phase 2 — Tests → DoD-2, DoD-4

- [ ] Integration: soft-fail attach → CSV present + **header equals** `EXPORT_ATTACHMENTS_CSV_HEADER`.
- [ ] Mode A: promoted winner + loser attach rows.
- [ ] Row-cap truncated marker.
- [ ] Ledger `off` still classifies exit 64 from counters.
- [ ] Ledger init fail → non-success / fail-closed (not silent continue).

## Phase 3 — Docs + finalize → DoD-5, DoD-6

- [ ] CLI help + operator note (CSV path, path-mode).
- [ ] Close `D-0073-eml` in `docs/deferred.md`.
- [ ] Write `review.md`; mark conductor **Completed**; commit ledger TX.

---

## Handoff notes

- DTO in engine, sink in CLI — never pass `AttachLedgerSink` into `eml_pack`.
- Do not change eml_pack MIME layout.
- Can run in parallel with 0088.
