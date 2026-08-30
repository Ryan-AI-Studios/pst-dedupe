# Track Completion Audit — 0112-ReviewWindow

## Verdict: FAIL

## Scope Reviewed

Fresh read-only review of `track/0112-review-window` working tree versus `origin/main`. Read `spec.md`, `plan.md`, implementation, tests, permissions, routes, CI, and relevant matter-core helpers.

## Requirement and DoD Matrix

| Item | Status | Evidence / gap |
|---|---|---|
| DoD-1 | Met in source | Three-pane route, separate privilege controls, keyboard overlay, coding token, queue route preserved. HITL not independently performed; owner-local. |
| DoD-2 | Partial | Host family/neighbor behavior and tests exist. Family confirmation is incorrect for families over 100 and can fail for no code changes. |
| DoD-3 | Partial | Host privilege ordering, basis validation, defaults, and description preservation are present. UI still misroutes log notes into privilege descriptions. |
| DoD-4 | Met | CAS length/prefix reads, lossy UTF-8, block-aware HTML stripping, honest empty/truncated responses, and tests are present. |
| DoD-5 | Reported met / partially observed | Registrations, capabilities, queue propagation guard, and source checks are present. `cargo fmt --all --check` passed. Targeted test rerun was blocked by read-only access to `target\debug\.cargo-lock`; supplied gates are reported, not independently observed. |
| DoD-6 | Not independently verifiable | Orchestrator-owned governance/HITL item. Not used to fail engineering completion. |

## Findings

[P1] Log notes overwrite privilege claim descriptions on the privilege-on path  
Confidence: High  
Requirement: DoD-3; §3.3 and §3.6 require privilege description and log note to remain separate.  
Location: `crates/dedupe-chrome/ui/src/pages/review_window.rs:373`  
Problem: When privilege is being added, the UI sends the log-note text as `privilege_description`, then separately saves the same text as a note. The host correctly preserves descriptions only when the field is omitted, but the UI supplies it whenever a note exists.  
Evidence: `privilege_description: Some(note_to_save.clone())` at lines 373–376; note persistence follows at lines 409–417.  
Failure scenario: Enter a legal log note while turning privilege on. The note is incorrectly stored as the privilege claim description.  
Correction: Pass `privilege_description: None` from this UI; add a distinct description field only if editing claim descriptions is intended.  
Verification: Add an end-to-end host/UI contract test proving a log note does not alter an existing privilege description.  
Deferrable: No

[P2] Family privilege preview undercounts families larger than 100  
Confidence: High  
Requirement: §3.5 requires preview IDs to represent the expanded family; capped DTOs may only be used when the family size is at most 100.  
Location: `crates/dedupe-chrome/ui/src/pages/review_window.rs:320-339`  
Problem: The confirmation preview always sends `d.family_members`, which is capped at 100, while the confirmation text uses the full `family_size`.  
Evidence: `family_members` is capped by `family_members_thin(fid, 100)`; the UI passes only those IDs to `review_codes_preview` at line 324.  
Failure scenario: A 101-member family with no privilege codes displays “Privilege would change on 100 family members,” while applying the operation affects 101.  
Correction: Preview the complete expanded set only when `family_size <= 100`; otherwise omit the numeric privilege preview or implement a safe count-based host preview.  
Verification: Add a >100-member family test asserting the confirmation does not report a capped count as complete.  
Deferrable: No

[P2] Family confirmation blocks saves with no code operation  
Confidence: High  
Requirement: Save & Next must persist pending document changes; family propagation applies to code operations.  
Location: `crates/dedupe-chrome/ui/src/pages/review_window.rs:320`  
Problem: Any propagated family with more than one member opens confirmation, even when `add` and `remove` are empty. Confirm then invokes `review_window_apply`, which rejects empty code operations.  
Evidence: The confirmation condition does not check `add`/`remove`; `matter-core` rejects both empty at `crates/matter-core/src/matter.rs:4923`.  
Failure scenario: Enable “Apply to family” and save only a note, or save an already-selected code. Confirmation appears, then Confirm returns `apply_codes requires at least one add or remove code id`.  
Correction: Require a pending code operation before family confirmation; save item-specific notes/privilege edits without invoking family code apply.  
Verification: Test note-only and no-op Save & Next with family propagation enabled.  
Deferrable: No

[P2] Privilege basis leaks between documents  
Confidence: High  
Requirement: Per-document privilege type state must reflect the loaded document.  
Location: `crates/dedupe-chrome/ui/src/pages/review_window.rs:184-200`  
Problem: `pending_basis` is updated only when the new document has a privilege claim. It is not reset when loading an unprivileged document.  
Failure scenario: Load a document with `work_product`, navigate to an unprivileged document, press `p`, and save. The second document is coded as `work_product` without the reviewer selecting that type.  
Correction: Reset `pending_basis` to the default before applying the newly loaded document’s privilege data.  
Verification: Add a document-to-document state isolation test for privilege basis.  
Deferrable: No

## Completeness Sweep

- Review route no longer contains the 0112 stub.
- Image 0114 and Produced 0113 placeholders are explicitly allowed.
- New commands are registered and permission files exist.
- No new `innerHTML`, `get_bytes_capped`, or `list_family_members` use was found in the review path.
- No production `unwrap()`/`expect()` was found in the changed host paths; test-only uses remain.
- CSP and workspace exclusion remain unchanged.

## Prior Finding Recheck

- Note draft crossing documents: fixed by clearing on load and after successful save.
- Withhold/basis wiping privilege descriptions: host-level preservation is fixed and tested, but the UI still overwrites descriptions when a log note accompanies privilege activation; remains open end-to-end.
- Family privilege preview missing: fixed for capped families, incomplete for families over 100.
- Queue “stub” wording: fixed.
- DoD-6 governance/HITL: orchestrator-owned and not treated as an engineering failure.

## Verification Evidence

Observed:

- `cargo fmt --all --check` passed.
- `git diff --check` passed.
- Targeted `cargo test -p dedupe-chrome --locked` could not run because read-only access denied `C:\dev\Dedupe\target\debug\.cargo-lock`.

Reported by orchestrator:

- 62 `dedupe-chrome` tests passed.
- `dedupe-chrome` clippy passed.
- Prior workspace gates and Ledgerful verification passed.

Ledgerful status/impact writes were unavailable under read-only restrictions; AI-Brains was unavailable because no vault key was configured.

## Deferred Candidates

None. All findings are P1/P2 and are not deferrable.

## Completion Decision

Engineering completion is not approved. Fix the four findings, rerun the supplied gates, and request a fresh re-review.