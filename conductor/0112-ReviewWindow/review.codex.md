# Track Completion Audit — 0112-ReviewWindow

## Verdict: FAIL

## Scope Reviewed

Audited `origin/main..working tree` on `track/0112-review-window`, including unstaged and untracked product files. Read all of:

- `conductor/0112-ReviewWindow/spec.md`
- `conductor/0112-ReviewWindow/plan.md`

## Requirement and DoD Matrix

| Area | Result |
|---|---|
| Three-pane Tauri/Leptos review window | Implemented and routed |
| Read-first catalog and review metadata | Implemented |
| CAS body cap, HTML stripping, UTF-8 handling | Implemented |
| Neighbor navigation and dropout handling | Implemented |
| Privilege pre-check → apply → upsert sequence | Implemented |
| `ensure_item_privilege_conn` unchanged | Confirmed |
| Queue filter propagation via `?filter=` | Implemented |
| No 0117 virtualization changes | Confirmed |
| No `innerHTML`, `get_bytes_capped`, or coral color | Confirmed |
| Family thin loading and orphan handling | Implemented |
| Family privilege preview before apply | **Missing** |
| Note lifecycle across navigation | **Defective** |
| Existing privilege description preservation | **Defective** |
| DoD-1 through DoD-5 | Partially met; issues below |
| DoD-6 governance/completion artifacts | **Unmet** |
| DoD-6 HITL EXE smoke | Owner-local; not failed solely for absence |

## Findings

[P1] Note draft crosses document boundaries

Confidence: High  
Requirement: Review notes must be saved against the intended document without contaminating subsequent review items.  
Location: `crates/dedupe-chrome/ui/src/pages/review_window.rs:125,180-197,384-392`  
Problem: `note_draft` is not cleared or initialized when the route changes. `review_upsert_note` is subsequently called with the new `item_id` and `id: None`.  
Evidence: The document-load effect resets code and privilege state but never resets `note_draft`.  
Failure scenario: A note typed for item A is saved to item A, then remains in the textarea and is saved again to item B after Save & Next or manual navigation.  
Correction: Reset or intentionally hydrate the note draft on document changes, and add a navigation regression test.  
Verification: Test Save & Next and back/forward navigation with a non-empty note.  
Deferrable: No

[P1] Withhold or basis edits erase existing privilege descriptions

Confidence: High  
Requirement: `review_upsert_privilege` must update withhold/basis without corrupting existing privilege data.  
Location: `crates/dedupe-chrome/ui/src/pages/review_window.rs:364-373`; `crates/dedupe-chrome/src/privilege_cmd.rs:53`  
Problem: Existing privilege descriptions are loaded but not retained in pending state. The UI sends `description: None`, and the host converts that to an empty description before upsert.  
Evidence: `unwrap_or_default()` replaces an omitted description with `""`.  
Failure scenario: An existing privilege claim with description “Legal advice” loses that description when only basis or withhold is changed.  
Correction: Preserve the existing description when omitted, or send the current description explicitly; add a regression test.  
Verification: Apply basis-only and withhold-only edits to a claim with a non-empty description.  
Deferrable: No

[P2] Family privilege-change preview is not wired into the review window

Confidence: High  
Requirement: When family propagation is enabled, privilege-change preview must use the expanded family member set and be cancellable before any write.  
Location: `crates/dedupe-chrome/ui/src/pages/review_window.rs:247-423,784-792`  
Problem: The window displays a confirmation bar but never invokes the existing `review_codes_preview` command. It does not present the required expanded privilege-change preview before applying.  
Evidence: `review_codes_preview` is used by the queue, but there is no call from `review_window.rs`.  
Failure scenario: A reviewer enables family propagation and applies a privilege code without seeing the required expanded privilege impact.  
Correction: Invoke and render the expanded-set preview before confirmation; ensure cancel performs no write.  
Verification: Test family propagation with privilege addition/removal and cancellation.  
Deferrable: No

[P1] Engineering completion governance is incomplete

Confidence: High  
Requirement: DoD-6 requires review evidence, Completed registry state, closed D-0112, ledger provenance, and unblocking metadata.  
Location: `conductor/0112-ReviewWindow/review.md` missing; `conductor/conductor.md:295`; `conductor/ROADMAP.md:429`; `conductor/sequencing.md:253`; `docs/deferred.md:921`  
Problem: The track remains Ready/In Progress, D-0112 remains open, and no canonical `review.md` or implementation ledger commit is present.  
Evidence: `review.md` does not exist; governance files still identify 0112 as incomplete.  
Failure scenario: The implementation cannot be recognized as a completed Ledgerful track or safely unblocks dependent tracks.  
Correction: Complete the canonical review artifact, update registry/roadmap/sequencing/deferred state, and establish verified ledger provenance.  
Verification: `ledgerful verify`, clean completion metadata, and committed provenance.  
Deferrable: No

[P3] Queue help text still calls the removed route a stub

Confidence: High  
Requirement: No stale placeholders or inaccurate user-facing claims remain.  
Location: `crates/dedupe-chrome/ui/src/pages/queue.rs:646`  
Problem: The shortcut text says “open review window stub (0112)” even though the stub was deleted and the real review window is wired.  
Correction: Update the help text to refer to the review window without “stub.”  
Verification: Search UI sources for stale 0112 stub references.  
Deferrable: No

## Completeness Sweep

The main implementation is present and reachable:

- The stub module was deleted and the route now renders `ReviewWindow`.
- All five Tauri commands are registered and dispatched through blocking workers.
- Catalog loading is read-first.
- Review metadata, family cards, neighbors, body retrieval, notes, privilege updates, and queue query propagation are wired.
- The privilege write order is correct: basis pre-check, code application, then privilege upsert with compensation.
- `ensure_item_privilege_conn` is unmodified.
- Body handling uses `cas_len` and `read_cas_prefix`; no `get_bytes_capped` or `innerHTML`.
- HelloWorld whitespace behavior is covered.
- No 0117 queue virtualization changes were found.

The note lifecycle, privilege-description preservation, family preview, stale copy, and completion governance remain incomplete.

## Wiring and Regression Review

The core production path is wired end to end:

`queue → review_doc_href(?filter=...) → ReviewWindow/use_query_map → review_document → review_window_apply → matter-core`

The primary regressions are state-lifecycle and review-safety issues rather than unreachable code. Existing reported tests do not cover cross-document note contamination, description preservation, or family privilege preview behavior.

## Verification Evidence

Observed:

- `cargo fmt --all --check` passed.
- `git diff --check origin/main` passed, with only a CRLF warning in `html_strip.rs`.

Reported by the implementer:

- `matter-core` family and neighbor tests passed.
- `dedupe-chrome` tests: 61 passed.
- `dedupe-desk` check passed.
- Targeted clippy passed.

Independent cargo reruns were blocked by the read-only environment because Cargo could not open `target\debug\.cargo-lock`. Ledgerful status/doctor also failed because its database could not be opened. No Ledgerful verification signal was therefore available.

## Deferred Candidates

None. The identified issues are required for completion or are straightforward corrections, not difficult non-blocking P3 work.

## Completion Decision

FAIL.

DoD-6 is not being failed because the owner-local EXE smoke was not launched. The track fails because engineering completion requirements remain unmet: note and privilege-data correctness defects, missing family privilege preview, stale stub wording, and incomplete completion/provenance governance.