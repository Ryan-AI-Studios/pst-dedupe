# 0122 — ProcessFoldResiduals — Plan

> Placeholder minted 2026-08-31 from PR **#123** Bugbot. Expand with
> `/plan-track 122`. Do **not** implement from this file. Do **not** fold
> these into **0117**. Cancelled produce stays on **0119**.

## Phase 0

- [ ] Re-read live `crates/dedupe-chrome/ui/src/pages/process.rs`
      (`extract_all` ~382–428, `is_orphan_running` ~133, job `For` ~546–579).
- [ ] Re-read PR #123 comments extract-all Busy + live-jobs-as-orphans.

## Phase 1

- [ ] Extract-all: do not `extract_queue.set(Vec::new())` on Busy; skip a
      second extract-all while a queue is draining.
- [ ] Job row orphan/active from current `progress` signal, not mount snapshot.

## Phase 2

- [ ] Host/UI tests or HITL notes; `review.md`; registry Completed; ledger.
