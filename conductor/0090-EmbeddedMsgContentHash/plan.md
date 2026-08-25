# 0090 — Embedded Message Content Hash — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-24):** re-baseline — method 5 is subnode parse; include header_hash; not Relativity parity; CLI `attach_content_hash` is the fill path — see `spec.md` §2.8.

> **Ledger:** `ledgerful ledger start crates/pst-reader --category FEATURE --message "0090 embedded-msg identity hash (reader + CLI digest + engine preimage)"`

---

## Phase 0 — Design lock → DoD-1 (partial)

- [x] Re-read 0086 Choice B + `hash_attachment_stream` unread-on-open-fail.
- [x] Lock preimage bytes exactly as `spec.md` §2.4–2.5 (header included; attach index order; inline flag; depth 0 = this embed).
- [x] Lock parser split: rfc822 (method 1) vs subnode (method 5).
- [x] Lock depth-limit sentinel domain sep (not raw blob).
- [x] Confirm D-0067 remains **export** residual.

## Phase 1 — Reader identity load → DoD-1, DoD-2

- [x] Implement budgeted `read_embedded_message_identity` (name as convenient).
- [x] Fail closed → caller unread sentinel.
- [x] Unit tests on synthetic writer fixture with method-5 attach if available; else generate in-test.
- [x] No production `unwrap`/`expect`.

## Phase 2 — Preimage + CLI wire → DoD-1, DoD-3

- [x] `embedded-msg-hash/v1` helper (engine or CLI module with tests).
- [x] Wire `attach_content_hash.rs` / `scan.rs` for method 5 + rfc822.
- [x] Golden: nested subject change splits; nested body change splits; depth cap.

## Phase 3 — Docs + honesty → DoD-4, DoD-5

- [x] Operator docs: **not Relativity parity**; why recursive-in-parent.
- [x] Surface unparsed / depth-cap flags on keep-set/report path as specified.
- [x] Close/narrow `D-0086-embedded-email-hash`; leave `D-0067-embedded-depth` open.
- [x] Note optional operator-local embedded-msg PST smoke in `review.md`.

## Phase 4 — Finalize → DoD-6

- [x] `review.md`; conductor **Completed**; ledger commit.
- [x] Handoff to 0091: digest API now includes embedded-aware results — unify against this API.

---

## Handoff notes

- Do not ship as default identity.
- Do not pull full nested extract / production materialize of children.
- Prefer completing **before 0091**.
- Estimate is **reader + hasher + CLI**, not hasher-only.
