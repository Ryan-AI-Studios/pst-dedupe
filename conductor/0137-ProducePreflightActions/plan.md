# 0137 — Produce pre-flight actions — Plan

> Status: **Ready — not started**. Acknowledge live “Open in review”; only add remaining jumps.
> Fold-in 2026-09-03: `opencode-review.md` + `agy-review.md`.

> **Ledger:** `ledgerful ledger start 0137-produce-preflight --category FEATURE --message "Wire produce extras without item_id to Set/QC/protocol"`

---

## Phase 0 — Pin extras kinds → DoD-2

- [ ] Re-read `chrome_extras`, `ChromeExtra`, `review_doc_href`, produce extras/findings `For` blocks.
- [ ] Confirm step nav uses plain `<a href="#step-N">` (not `<A>`). Protocol pane still has no `id`.

## Phase 1 — Kind dispatch → DoD-1 / DoD-2

- [ ] Helper: `item_id` Some → `review_doc_href` (Leptos `<A>`); `empty_selection` → `#step-1-set`; `privilege_log_blank` → `#privilege-protocol`; `qc_gate` → **no hash** (Re-run QC stays); unknown + no id → `None`.
- [ ] In-page jumps use **plain `<a>`** (same as step nav). Do not use `<A>` for hashes.
- [ ] Add `id="privilege-protocol"` on `<div class="produce-protocol">`. `include_str` lock that id.
- [ ] Do not add a Review QC-queue route. Do not change Finalize disable rules. Do not touch `app.css` unless required (D-0125-dead-css remains).
- [ ] Unit tests for the helper including `unknown_kind_none`; existing 0119 latch + 0125 un-wizard tests still pass.

## Phase 2 — Finalize → DoD-3

- [ ] `review.md`; ledger commit.

## Handoff

- Do not invent ACME pre-flight rows.
- Do not treat this track as greenfield Open-in-review (already shipped).
