# 0124 — ReviewQueueChrome — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Ledger (execute):** After owner git-commits this Ready spec/plan if still dirty, `ledgerful ledger start 0124-review-queue-chrome --category FEATURE --message "Queue ellipsis, 244px rail, Go-to slot, truthful row range"`

---

## Phase 0 — Precondition / API gate → DoD-4

- [x] Re-verify `SCHEMA_VERSION == 41`. Re-read `app.css` `.queue-row` / `.queue-viewport`, `queue.rs` cells + chips + footer, `queue_window.rs` (`ROW_HEIGHT` 32), `shell.rs` empty slots, `app.rs` Ctrl+K, host `src/queue.rs` `QueueRow`.
- [x] Re-read mock `.panes-2` 244px + `.doc-table` nowrap (research only). Do **not** port privilege pills.
- [x] Confirm last PRs (#140–#137) still have no product findings. Do **not** edit `visible_range`. Do **not** implement 0125–0126.

## Phase 1 — Collision → DoD-1

- [x] CSS: `minmax(0,…)` tracks; cell `overflow: hidden; text-overflow: ellipsis; white-space: nowrap`. From/Subject/Custodian `title` = display string. Control# **keeps** `title=item id`. Extras grid too.
- [x] Helpers + ui unit tests: `display_from`, family `"— attachment"` / parent copy on-page. `include_str` CSS lock for ellipsis.
- [x] Do **not** wrap; do **not** change `ROW_HEIGHT`.

## Phase 2 — Rail / toolbar / Go-to / range / bulk → DoD-2, DoD-3

- [x] 244px rail: Unreviewed / Privileged / Responsive map to existing presets; saved searches move here; Needs decision / Redaction QC / Consistency inert count 0 + “no filter yet” (not clickable empty corpus).
- [x] Toolbar title = active name + `{total} docs`. Rename Save → **Save search**. `aria-label` on the queue page.
- [x] **Do not** pass slot children from `wrap_review`. Provide `QueueChromeCtx` (`queue_range` + `goto_request`) **above** `MatterShell` in `wrap_review`. Page writes `last_fetch_meta` into `queue_range` and consumes Go-to. Slot UI reads ctx. `wrap_review_window` does **not** provide ctx / does **not** mount `#queue-goto`.
- [x] `#queue-goto`; Ctrl+K focuses it when `#matter-search` is absent **and** the input is in the DOM. Integer = current-page `review_order` using the **page** `root_sig`; miss copy `Control# N not found in current page (Rows a–b)` from `last_fetch_meta`. Else existing subject contains / page substring.
- [x] StatusBar left: `Rows {offset+1}–{offset+fetched} of {total}` (0117 empty-page copy when needed). **Select page ({len})** with `p.rows` ids only + keep Tag… / privilege preview.
- [x] Viewport height from remaining pane (flex), not a magic 640 if it overflows the 0123 shell. Measure `#queue.client_height` on **mount and resize**; write `viewport_h` before the first `visible_range`. Do **not** wait for `on:scroll`. `queue_window.rs` formulas stay frozen.

## Phase 3 — Verify → DoD-4

- [x] `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml`
- [x] `cargo test -p dedupe-chrome` (0117 twin + 0122 Busy + 0123 shell locks)
- [x] trunk / chrome-ui still builds.

## Phase 4 — Finalize → DoD-5

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace` (or ledgerful verify --scope full)
- [x] CHANGELOG Unreleased sentence.
- [x] Write `review.md` (commands, HITL X500 + Next range + Enter opens window).
- [x] Update `../conductor.md` → **Completed**. Close `D-0124-review-queue-chrome`.
- [x] Commit the FEATURE ledger transaction.
- [ ] Owner HITL: release EXE.

---

## Handoff notes

- Single-exe / no-daemon. Unique-pst is **not** this page.
- **0125** / **0126** stay Proposed. **0117** math stays Completed.
- `git add -f` only if `conductor/0124-*` shows **untracked**.
- Do not `git add` stray repo-root `agy-review.md` or `fixtures/keep_set_summary.json`.
