# Track Completion Audit — 0111-ReviewQueueFirstPass

## Verdict: FAIL

## Scope Reviewed

Working tree on `track/0111-review-queue-first-pass` versus `origin/main`, including uncommitted product, conductor, permission, UI, and test files. Read all of `spec.md`, `plan.md`, internal review files, changed host/UI code, CI, permissions, and relevant matter/search APIs.

## Requirement and DoD Matrix

| Requirement | Status | Evidence / gap |
|---|---|---|
| Queue route replaces 0110 stub | Partial | Route, Continue review, default Unreviewed chip, tabs, and 0112 stub are wired. Keyboard/focus defects remain. |
| Thin filtered queue and honest totals | Partial | Host uses `FilterSpec`, FTS composition, bounded pages, and filtered counts. UI displays `0 in queue` on command errors. |
| Family sizes | Met | `Matter::family_sizes` is matter-scoped, chunked, and does not extend `ReviewListRow`. |
| Codes, privilege distinction, extras | Partial | First-pass coding and extras are wired; custom responsiveness codes are mapped incorrectly. |
| Six Tauri commands and capabilities | Met | Registered, worker-backed, and permissioned; no `fs:default`. |
| Virtualization | Partial | Host math and bounded UI window exist. Release EXE/HITL DOM evidence is absent. |
| Saved searches | Partial | Upsert/list works, but named chips show no required total. |
| Keyboard and bulk tagging | Partial | Bulk path is wired, but event bubbling breaks controls and preview has an add/remove ordering defect. |
| DoD-5 verification | Unmet/not verifiable | `cargo fmt` passed. Targeted cargo test was blocked by read-only target lock access; full workspace test was not run. |
| DoD-6 recording/provenance | Unmet | No `review.md`; registry is Ready/In Progress rather than Completed; final HITL is pending; Ledgerful status is unavailable. |

## Findings

### [P1] Track completion and required final gates are not recorded

Confidence: High  
Requirement: DoD-5, DoD-6  
Location: [`spec.md`](<C:\dev\Dedupe\conductor\0111-ReviewQueueFirstPass\spec.md:356>), [`plan.md`](<C:\dev\Dedupe\conductor\0111-ReviewQueueFirstPass\plan.md:50>), [`conductor.md`](<C:\dev\Dedupe\conductor\conductor.md:294>)

Problem: The required `review.md` is absent. The registry remains Ready/In Progress, the plan’s finalization items are unchecked, release EXE HITL is pending, and the full workspace test has not been run.

Evidence: `Test-Path` showed no track `review.md`; git status shows an uncommitted dirty tree. Ledgerful status/doctor could not open its database under read-only restrictions.

Failure scenario: The track cannot be truthfully marked complete or unblock 0112 with the required evidence and provenance.

Correction: Complete the workspace gate and owner HITL, create the canonical review artifact, reconcile all registry statuses to Completed, and commit the required FEATURE ledger transaction.

Verification: Re-run all final gates, Ledgerful verification/status, and the HITL checklist.

Deferrable: No

### [P2] Named saved-search chips omit required totals

Confidence: High  
Requirement: Spec §3.6  
Location: [`queue.rs`](<C:\dev\Dedupe\crates\dedupe-chrome\ui\src\pages\queue.rs:381>), [`saved.rs`](<C:\dev\Dedupe\crates\dedupe-chrome\src\saved.rs:15>)

Problem: Named saved-search chips render only `{name}`. `SavedSearchDto` and `saved_searches_list` carry no applied/live total, so the UI cannot display “name + total after apply.”

Failure scenario: Counsel sees a saved-search chip but cannot see its result count as required.

Correction: Return and render the live count after applying each saved search, with honest FTS-unavailable handling.

Verification: Add a saved-search chip integration/UI check asserting name and total.

Deferrable: No

### [P2] Unknown responsiveness codes are shown as raw internal keys

Confidence: High  
Requirement: Spec §3.4: unknown responsiveness codes must display `—`  
Location: [`queue.rs`](<C:\dev\Dedupe\crates\dedupe-chrome\src\queue.rs:187>)

Problem: Any code in the `responsiveness` group other than the three defined keys is emitted as its raw key via `other.to_string()`.

Failure scenario: A custom or historical responsiveness code appears as an unsupported internal identifier instead of the required honest dash.

Correction: Map only `responsive`, `not_responsive`, and `needs_second_look`; map all others to `None`.

Verification: Add a test with an unknown active responsiveness-group code.

Deferrable: No

### [P2] Queue keyboard handling intercepts controls and rows are not focusable

Confidence: High  
Requirement: Spec §§3.7 and 3.9  
Location: [`queue.rs`](<C:\dev\Dedupe\crates\dedupe-chrome\ui\src\pages\queue.rs:236>), [`queue.rs`](<C:\dev\Dedupe\crates\dedupe-chrome\ui\src\pages\queue.rs:721>), [`app.css`](<C:\dev\Dedupe\crates\dedupe-chrome\ui\styles\app.css:23>)

Problem: The section-level handler processes bubbled `Enter`/Space events from buttons and checkboxes, while only text fields are excluded. Rows have no `tabindex`, and there is no row-specific `:focus-visible` rule.

Failure scenario: Pressing Enter on “Next page” can open the current document stub; pressing Space on the Lead/QC checkbox can toggle row selection instead of the checkbox. Rows cannot be reached through normal keyboard tab navigation.

Correction: Scope shortcut handling to the queue/row focus context, stop propagation from controls, and make rows keyboard-focusable with visible focus styling.

Verification: Browser/UI tests for control activation, row focus, arrows, Enter, and Space.

Deferrable: No

### [P2] FTS errors render as an apparently empty queue

Confidence: High  
Requirement: Spec §§3.3 and 3.4; no silent `total=0` fallback  
Location: [`queue.rs`](<C:\dev\Dedupe\crates\dedupe-chrome\ui\src\pages\queue.rs:650>), [`queue.rs`](<C:\dev\Dedupe\crates\dedupe-chrome\ui\src\pages\queue.rs:795>)

Problem: On `fts_unavailable` or any queue error, the UI clears `page`, then renders “0 in queue” and a footer total of zero alongside the error.

Failure scenario: A missing/stale FTS index is visually indistinguishable from a genuinely empty queue, despite the host correctly returning `fts_unavailable`.

Correction: Render an unavailable/error state without a zero count unless a successful response explicitly reports `total = 0`.

Verification: UI test a missing-index response and assert no zero-result fallback is shown.

Deferrable: No

### [P2] Privilege preview disagrees with apply order for overlapping add/remove IDs

Confidence: High  
Requirement: Spec §3.8; preview must model the actual apply operation  
Location: [`codes.rs`](<C:\dev\Dedupe\crates\dedupe-chrome\src\codes.rs:152>), [`matter.rs`](<C:\dev\Dedupe\crates\matter-core\src\matter.rs:4890>)

Problem: Preview removes privilege memberships first and adds them second. `Matter::apply_codes` adds first and removes second.

Failure scenario: If the same privilege code appears in both lists, preview can report no change when apply removes a membership, or report a change when apply leaves it unchanged.

Correction: Simulate the exact core operation order or reject overlapping add/remove IDs before preview.

Verification: Add tests for present and absent membership with the same code in both lists.

Deferrable: No

## Completeness Sweep

- Intentional `ReviewDocStub` remains for 0112; the old 0110 `ReviewStub` was removed.
- No 60k DOM strategy or infinite-scroll append path found.
- Six commands are registered and generated permissions exist.
- No `fs:default`; CSP remains unchanged.
- No production `unwrap()`/`expect()` found in the changed runtime paths; test helpers use them.
- No client PST was added by this track. An unrelated untracked `fixtures/keep_set_summary.json` exists and should not be staged with this work.

## Wiring and Regression Review

The primary queue path is wired end to end:

`ReviewQueue` → Tauri invoke → blocking worker → `Matter`/`matter-search` → filtered thin rows → codes/family/extras enrichment → bounded `visible_range` rendering.

Bulk tagging is also wired through preview → optional confirmation → `apply_codes` with actor `chrome` and family propagation forced off.

The remaining defects are in UI honesty/accessibility and preview equivalence, not missing command registration or matter-core boundaries.

## Verification Evidence

Observed:

- `cargo fmt --all --check`: passed.
- Direct source/config audit: completed.
- `cargo test -p dedupe-chrome`: could not start because `target\debug\.cargo-lock` was inaccessible under the read-only sandbox.
- `ledgerful scan --impact`, `ledgerful ledger status`, and `ledgerful doctor`: unavailable because Ledgerful could not open/write its database under read-only restrictions.

Reported but not independently observed:

- Clippy success.
- `cargo test -p matter-core --test family_sizes`.
- `cargo test -p dedupe-chrome` with 40 tests.
- `cargo check -p dedupe-desk`.
- wasm UI check and trunk build.

Not complete:

- Full `cargo test --workspace`.
- Release EXE owner HITL.
- Canonical `review.md`.
- Final Ledgerful/provenance confirmation.

## Deferred Candidates

None. All findings are P1/P2 or required completion work; no difficult, non-blocking P3 qualifies for deferral.

## Completion Decision

FAIL. Fix the P1/P2 findings, complete the missing workspace/HITL/provenance gates, then run a fresh completion review.