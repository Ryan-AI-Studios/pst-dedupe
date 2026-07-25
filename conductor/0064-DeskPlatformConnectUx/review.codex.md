# Track Completion Audit — 0064-DeskPlatformConnectUx

## Verdict: FAIL

## Scope Reviewed

Read-only audit of the uncommitted working tree on `feat/0064-desk-platform-connect-ux` versus `main`, including `spec.md`, `plan.md`, Desk/service code, docs, deferred ledger, prior reviews, and verification artifacts.

## Requirement and DoD Matrix

| Requirement / DoD | Result | Evidence |
|---|---|---|
| Connect/session | Met nominally | Health check, password login, in-memory bearer session, banner, logout |
| Mode fail-closed | Partial | Normal guards exist, but async Connect/open race permits hybrid state |
| Remote list/body/codes | Partial | Core path wired; coding state is not item-scoped |
| OCC 409 UX | Partial | Draft retained, but server snapshot lacks codes/notes and errors are swallowed |
| SSO handoff | Met | Loopback listener, system browser, one-time exchange code, no clipboard paste |
| Solo produce UX | Met | Profile picker, Bates start, profile validation, QC pre-flight |
| Networking | Met with residual | Single body worker, queue drain, latest-wins generation gate |
| DoD-1 Connect | Met | Functional implementation present |
| DoD-2 Remote mutate + 409 | Partial | Cross-item draft/result handling is unsafe |
| DoD-3 Mode fail-closed | Partial | Race violates dual-open invariant |
| DoD-4 Produce UX | Met | Required profile/Bates/pre-flight path present |
| DoD-5 Solo regression | Met statically | Existing Solo paths remain reachable |
| DoD-6 SSO | Met | Automatic loopback handoff implemented |
| DoD-7 Networking | Met | Single-flight blocking fallback is wired |
| DoD-8 Tests | Partial | Gates reported green, but key client/service paths lack end-to-end coverage |
| DoD-9 Docs | Met | Operator, Desk, service, feature, and deferred docs align functionally |
| DoD-10 Recorded | Unmet | No canonical `review.md`; registries remain `Ready`; ledger commit not evidenced |

## Findings

### [P1] Async Connect/open race can create a hybrid Solo + Connected state

Confidence: High  
Requirement: §3.3, DoD-3  
Location: [app.rs](/C:/dev/Dedupe/crates/dedupe-desk/src/app.rs:2292), [app.rs](/C:/dev/Dedupe/crates/dedupe-desk/src/app.rs:501), [app.rs](/C:/dev/Dedupe/crates/dedupe-desk/src/app.rs:849)

Problem: Local matter controls remain usable while the Connect dialog or worker is active. `apply_connect_result` and `poll_matter_op` do not re-check the opposite mode before committing state.

Evidence: Connect only checks `matter_root` when opening the dialog; `set_matter` and successful Connect independently assign their state.

Failure scenario: Start Connect with no local matter, open/create a local matter before login completes, then receive a successful login. Both `matter_root` and `connected_session` become populated.

Correction: Disable all local open/create actions while Connect is open or busy, and enforce invariant checks again when applying Connect and matter-operation results.

Verification: Add a race/state test covering Connect pending plus local open completion in either order.

Deferrable: No

### [P1] Remote coding drafts and in-flight results are not item-scoped

Confidence: High  
Requirement: §3.4, §3.4.1, DoD-2  
Location: [remote_review_ui.rs](/C:/dev/Dedupe/crates/dedupe-desk/src/remote_review_ui.rs:347), [remote_review_ui.rs](/C:/dev/Dedupe/crates/dedupe-desk/src/remote_review_ui.rs:389), [remote_review_ui.rs](/C:/dev/Dedupe/crates/dedupe-desk/src/remote_review_ui.rs:241)

Problem: A single global `codes_draft`, conflict state, and codes result channel are reused across selections. Navigation does not clear or key the draft, and result polling does not verify that the result belongs to the current item.

Failure scenario: Enter codes for item A, navigate to item B, and click Apply; A’s draft is submitted to B. If A’s request completes after navigation, its success/conflict result can also overwrite B’s UI state.

Correction: Key drafts, conflict metadata, and requests by item ID/generation; ignore stale results and preserve drafts per item or require explicit discard.

Verification: Test A→B navigation with retained drafts and delayed success/409 responses.

Deferrable: No

### [P2] 409 server-state refresh is incomplete and silently hides auth failures

Confidence: High  
Requirement: §3.4.1 rules 3–4; §3.3  
Location: [remote_review_ui.rs](/C:/dev/Dedupe/crates/dedupe-desk/src/remote_review_ui.rs:431), [routes.rs](/C:/dev/Dedupe/crates/matter-service/src/routes.rs:104)

Problem: The 409 path calls `get_item(...).ok()`, discarding refresh errors. The returned `ItemThin` contains only metadata and `review_version`; no current codes/notes summary is fetched or displayed.

Failure scenario: The session expires between the 409 response and snapshot GET. The 401 is swallowed, so the app remains Connected. More generally, the conflict panel tells the operator to review server state without showing current coding state.

Correction: Propagate typed snapshot errors, especially Unauthorized, and expose/render the required current codes/notes summary.

Verification: Integration test 409 followed by 401; conflict test asserting server summary visibility.

Deferrable: No

### [P3] No Desk-to-service integration coverage for login and 409 flow

Confidence: Medium  
Requirement: §3.9, DoD-8  
Location: `crates/dedupe-desk/src/remote_client.rs` tests; `crates/matter-service/tests/integration.rs`

Problem: Tests cover DTO builders and pure state helpers, but not the actual Desk client healthz/login sequence or the production client 409 path against a mock/in-process router.

Failure scenario: API DTO or route drift can pass unit tests and fail only during a pilot.

Correction: Add a focused in-process/mock HTTP test for healthz + login and codes 409 handling.

Verification: Run the new integration tests with the existing targeted gates.

Deferrable: Yes, if the harness is treated as difficult non-blocking test infrastructure work.

### [P3] SSO cancellation leaves the listener worker polling until timeout

Confidence: Medium  
Requirement: §3.5, §3.7 resource hygiene  
Location: [connect.rs](/C:/dev/Dedupe/crates/dedupe-desk/src/connect.rs:241)

Problem: Cancel drops the receiver but does not signal or unblock the listener. The worker can retain its ephemeral port for up to three minutes.

Correction: Add a cancellation flag or self-connect to unblock the listener promptly.

Verification: Cancel SSO and assert worker/listener termination.

Deferrable: Yes

### [P3] Active body request is single-flight but not truly abortable

Confidence: Medium  
Requirement: §3.7.1  
Location: [remote_review_ui.rs](/C:/dev/Dedupe/crates/dedupe-desk/src/remote_review_ui.rs:475)

Problem: The single worker prevents an unbounded thread pool and drops queued stale jobs, but an already-running blocking request can continue until its 30-second timeout.

Correction: Future async `reqwest` cancellation or an abortable transport.

Verification: Slow-server navigation test measuring cancellation latency.

Deferrable: Yes; current single-flight/latest-wins fallback remains bounded.

## Completeness Sweep

No core Connect, SSO, remote review, or produce stubs were found. Clipboard bearer paste is absent. Intentional residuals are documented.

The OCC snapshot `.ok()` path is a real silent fallback. The working tree also has two documentation trailing-whitespace findings from `git diff --check`, treated as style-only.

## Wiring and Regression Review

Nominal wiring is complete:

`Connect → healthz/login or SSO exchange → ConnectedSession → banner → remote list/body/codes → OCC conflict UI`

`Solo matter → production profiles → Bates start → profile/body/QC pre-flight → produce job`

The main regressions are the asynchronous mode race and cross-item remote coding state. No schema migration was introduced.

## Verification Evidence

Observed artifacts:

- `.ledgerful/reports/latest-verify.json` records successful `cargo fmt`, workspace clippy, and workspace tests.
- Orchestrator-reported gates: 149 Desk tests passed; matter-service unit/integration passed; touched-crate clippy passed.
- `ledgerful ledger status --compact` was unavailable: `unable to open database file`.
- No commands were re-run by this read-only audit.

## Deferred Candidates

Only the P3 items above qualify as possible deferrals after orchestrator validation:

- Mock/in-process Desk/service integration tests
- Prompt SSO listener cancellation
- True abortable body transport

The P1/P2 findings and DoD-10 closeout cannot be deferred.

## Completion Decision

The implementation is substantial and most nominal paths are wired, but the track is not complete. Fix the hybrid-mode race, item-scope remote coding state, and OCC snapshot/auth handling; then re-run verification and complete `review.md`, registry status, and Ledgerful closeout.

**Final verdict: FAIL**