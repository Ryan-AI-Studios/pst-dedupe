# 0117 — QueueVirtualizationResiduals — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Ledger:** `ledgerful ledger start 0117-queue-virtualization --category BUGFIX --message "Queue header/spacer, empty-page honesty, arrow scroll_top"`

---

## Phase 0 — Precondition / API gate → DoD-4

- [ ] Re-verify `SCHEMA_VERSION == 41`. Re-read live `ui/src/pages/queue.rs` (header markup ~732, empty branch ~703, arrow keys ~318). Re-read host + UI `queue_window.rs`. Confirm `visible_range` tests still match 32px / overscan 8.
- [ ] Re-read PR #113 comments `7e063d89`, `09489de5`, `7fdba78c`. Catalog `dbf432d2` stays declined (read-first already).
- [ ] Do **not** implement 0118–0122. Do **not** bump schema. Do **not** change `PAGE_LIMIT` / `ROW_HEIGHT` / `OVERSCAN`.

## Phase 1 — Helpers → DoD-4

- [ ] Host `crates/dedupe-chrome/src/queue_window.rs`: add `last_page_offset`, `offset_after_empty_page`, `scroll_top_to_reveal` + tests in spec §3.4. Keep `visible_range` unchanged.
- [ ] Mirror the same functions in `ui/src/queue_window.rs`. Host test: `include_str!("../ui/src/queue_window.rs")` parity with the host module (twin-drift).

## Phase 2 — Queue UI → DoD-1, DoD-2, DoD-3

- [ ] Header sibling **above** `#queue` / `.queue-viewport`, inside a `.queue-grid` (`.extras` on that parent). Remove header from translated `.queue-window`. Spacer = `fetched_len * ROW_HEIGHT` only.
- [ ] CSS: extras templates on `.queue-grid.extras`; `overflow-y: scroll` + `scrollbar-gutter: stable` on `.queue-header` **and** `.queue-viewport`. Do not rely on sticky-inside-transform.
- [ ] Dedicated **clamp Effect** (not render): `offset_after_empty_page`; write only when `Some(new) != offset`. Gap: banner + last good page; do not `page.set(None)` on empty-page. Clamp `current_idx` after shrink.
- [ ] Empty body copy only when `total == 0`.
- [ ] `reset_queue_navigation()` on pager **and** chips / keyword Enter / family / saved-search.
- [ ] Arrow keys: `scroll_top_to_reveal`; set signal **and** `#queue.scrollTop` **only when** the helper differs. Shift+ArrowDown unchanged (open-current).

## Phase 3 — Verify + coexistence → DoD-4, DoD-1

- [ ] `cargo test -p dedupe-chrome` (helpers + existing chrome tests).
- [ ] chrome-ui wasm / trunk still builds. `ui/` has no process-runner dep.
- [ ] HITL note for header-outside (no ui page-snapshot tests). Windows: header/row columns line up with classic scrollbar gutter.
- [ ] Review / Process / Produce / Home tabs still route. Do not edit those pages.

## Phase 4 — Finalize → DoD-5

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (or ledgerful verify --scope full)
- [ ] CHANGELOG Unreleased sentence.
- [ ] Write `review.md` (commands, HITL header-outside + scrollbar alignment, 0122 reminder).
- [ ] Update `../conductor.md` → **Completed**. Close `D-0117-queue-virtualization` in `docs/deferred.md`.
- [ ] Commit the BUGFIX ledger transaction.
- [ ] Owner HITL: release EXE, synthetic Unreviewed page, arrows + last-page bulk tag.

---

## Handoff notes

- Single-exe / no-daemon. Jobs stay off this page.
- Unique-pst is **not** this page.
- **0118–0122** stay Proposed. Do not steal PR #123 Process/Produce into this PR.
- `conductor/` new files need `git add -f` when the owner commits (directory is gitignored for untracked).
