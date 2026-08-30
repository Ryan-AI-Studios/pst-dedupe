# Track Completion Audit — 0111-ReviewQueueFirstPass

## Verdict: FAIL

## Scope Reviewed

Working tree `track/0111-review-queue-first-pass` versus `origin/main`, including spec/plan, prior reviews, host/UI code, permissions, CI, tests, and relevant `matter-core`/`matter-search` APIs. No files or Git state were modified.

## Requirement and DoD Matrix

| Requirement | Status | Evidence |
|---|---|---|
| Queue replaces stub and routes wire | Partial | Queue, tabs, Continue review, and 0112 stub are wired. Single-click row activation remains incorrect. |
| Honest counts/columns | Met | Filtered totals, 500 cap, family sizing, extras separation, privilege mapping, and error handling are implemented and covered by host tests. |
| Virtualization | Met / HITL pending | `visible_range` tests and bounded DOM window are present. Release EXE DOM evidence was not available. |
| Saved search, keyboard, bulk tagging | Partial | Prior fixes are present. Escape does not clear the bulk bar while focus guards are active. |
| Tests/CI | Partial evidence | `cargo fmt --all --check` passed. Targeted cargo test was blocked by `target\debug\.cargo-lock`; orchestrator reports 42 tests, clippy, workspace gates, and wasm checks passed. |
| DoD-6 governance | Partial / orchestrator-owned | No canonical `review.md`, Completed registry, final HITL, or committed ledger transaction. Not failed solely for this per instruction. |

## Findings

### [P2] Single-clicking a queue row does not open the review window

Confidence: High  
Requirement: Spec §2.3 and §3.2; row activation must land on the 0112 stub.  
Location: [queue.rs](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/queue.rs:771)

Problem: The row’s `click` handler only updates `current_idx`; navigation is attached only to `dblclick`.

Failure scenario: A counsel user clicks a row once and remains on the queue instead of opening `/matters/:id/review/:docId`.

Correction: Make a normal row click navigate, while preserving checkbox click isolation.

Verification: Browser test that one row click reaches the 0112 stub and checkbox clicks do not navigate.

Deferrable: No

### [P2] Escape does not clear the bulk bar from focused interactive controls

Confidence: High  
Requirement: Spec §3.7: Escape closes overlays / clears the bulk bar; Escape is explicitly exempt from the field focus gate.  
Location: [queue.rs](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/queue.rs:267)

Problem: When a field or interactive target is focused, Escape closes overlays and tag/confirm state but never clears `selected`. The bulk bar remains rendered because it is controlled by `selected`.

Failure scenario: Select rows, focus the keyword or save-name field, press Escape; the bulk bar and selection remain.

Correction: Handle Escape as the explicit exception and clear `selected` when the bulk bar is active.

Verification: Keyboard test with focus in keyword, save-name, checkbox, and tag controls.

Deferrable: No

## Prior FAIL Findings

All five claimed fixes were verified:

- P2-1: Named saved chips receive `(total)` only after successful fetch; errors do not write fake zero totals.
- P2-2: Responsiveness maps only `R`, `NR`, and `NSL`; unknown keys become `None`.
- P2-3: Interactive targets are skipped; rows have `tabindex="0"` and `:focus-visible`.
- P2-4: Error responses clear the page without rendering `0 in queue`; successful empty responses render zero.
- P2-5: Preview simulates add-then-remove in the same order as `apply_codes`, with overlap coverage.

## Completeness Sweep

No new placeholder, fake count, coral token, `fs:default`, raw SQL in chrome, client PST, or production `unwrap`/`expect` issue was found. The 0112 review-window stub is intentional and in scope.

## Verification Evidence

Observed:

- `cargo fmt --all --check`: passed.
- Existing test binary lists 42 `dedupe-chrome` tests.
- `cargo test -p dedupe-chrome --locked`: blocked by access denied opening `target\debug\.cargo-lock`.
- Ledgerful status failed because its database was unavailable; impact scan could not write reports under read-only mode.
- `ai-brains` context commands were unavailable because `AI_BRAINS_KEY` is missing.

## Completion Decision

tokens used
251,419
FAIL. Fix the two P2 interaction defects, then perform another fresh review.

