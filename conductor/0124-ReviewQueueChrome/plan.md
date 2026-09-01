# 0124 — ReviewQueueChrome — Plan

> Placeholder minted 2026-08-31 from `C:\dev\deviations.md` + owner note
> that **column text collides**. Expand with `/plan-track 124`. Do **not**
> implement from this file. Do **not** fold into **0117**.

## Phase 0

- [ ] Re-read live `ui/styles/app.css` `.queue-row` / `.queue-viewport` and
      `ui/src/pages/queue.rs` row cells (From, Subject, extras).
- [ ] Re-read mock `.doc-table` nowrap + `.doc-table-wrap` overflow.
- [ ] Confirm `ROW_HEIGHT == 32` and `visible_range` stay 0117.

## Phase 1 — Collision (DoD-1)

- [ ] Cell clip: `min-width: 0`, nowrap, ellipsis, `title` = full text.
- [ ] HITL long X500 From; extras grid.

## Phase 2 — Rail / toolbar / bulk / range

- [ ] 244px rail; queue title; bulk bar; status row range.
- [ ] Family-member dash copy; SMTP preference if cheap.

## Phase 3

- [ ] `review.md`; registry Completed; ledger.
