# Track Completion Audit — 0133–0137 Series V

## Verdict: FAIL

One P1 and three P2 findings remain open. No P0 or deferrable P3 findings were identified.

## Scope Reviewed

- Branch: `track/0133-0137-series-v`
- HEAD: `cc8857694457e56d5ac4b05d34a2112c0a73afc3`
- Scope: unstaged working tree
- Reviewed all five `spec.md` and `plan.md` files, the nine modified tracked files, and relevant runner, ingest, extraction, schema, capability, and Produce code.
- Schema remains 41; Tauri resolves to 2.11.5; Leptos remains 0.8.
- No files or Git state were modified.

## Requirement and DoD Matrix

| Requirement | Status | Evidence and tests | Gap |
|---|---|---|---|
| 0133 DoD-1 — preserve arithmetic; name leaves | Met | `unaccounted_for` body is unchanged; host emits exact `unextracted_psts`/`failed_unlogged`; UI renders basenames and honest footnote. Formula test observed passing. | None for arithmetic/naming. |
| 0133 DoD-2 — Extract remaining queues only the gap and drains | Partial | UI builds work from `pg.unextracted_psts`, preserves the Busy guard and single production queue wipe. UI suite passes. | Fast terminal jobs do not advance the drain; required queue-ID behavioral test is absent. Finding 1. |
| 0133 DoD-3 — fast ingest refreshes sources | Partial | `importing + idle/terminal` predicate exists and is tested. | It only works if the immediate page reload already captured an importing source. Runner acceptance precedes source insertion, leaving a race. Finding 1. |
| 0133 DoD-4 — recorded | Unmet | Registry currently says In progress/Ready; no `review.md`; ledger transaction not verifiable. | Expected after fixes and clean re-review. |
| 0134 DoD-1 — basename/status/size/honest progress | Met | File-only `symlink_metadata`, basename, status, optional size, and mapped-only progress are wired. | None. |
| 0134 DoD-2 — real Tauri drop and honest multi-file policy | Partial | Direct Tauri listener, host event fallback, ingest invocation, and cleanup path exist. | Fast terminal race, non-Busy multi-file omission, silent listener failures, and no release-HITL evidence. Findings 1, 3, and 4. |
| 0134 DoD-3 — retain Add buttons/default drag-drop/no OST/MBOX | Met | Both buttons remain; copy names PST/ZIP/Purview/folder; `dragDropEnabled` is omitted. | None. |
| 0134 DoD-4 — recorded | Unmet | No completion review or ledger completion yet. | Post-review gate. |
| 0135 DoD-1 — job source basename with kind fallback | Met | Checkpoint resolution is limited to ingest/extract; unambiguous ingest fallback and extract inventory fallback are implemented; kind remains visible as a subline. | None. |
| 0135 DoD-2 — orphan and em-dash locks | Met | Production has exactly one required orphan expression, one `{j.kind.clone()}`, one queue wipe; per-job Dupes/NIST/Families/Except. remain `—`. UI tests pass. | None. |
| 0135 DoD-3 — recorded | Unmet | Completion governance not yet written. | Post-review gate. |
| 0136 DoD-1 — honest empty state | Met | Empty groups show “No item_errors recorded”; no fabricated counts. | None. |
| 0136 DoD-2 — real groups and conditional Retry | Partial | Independent sample IDs, title fallback, failed/paused predicate, `process_resume`, and error surfacing are wired. | Fast Retry can complete without refreshing the failed row; required rendered visibility cases are not behaviorally tested. Finding 1. |
| 0136 DoD-3 — no vault or fake exclude | Met | Copy explicitly says exclude unavailable and encrypted stores fail closed. | None. |
| 0136 DoD-4 — recorded | Unmet | No completion review/ledger commit. | Post-review gate. |
| 0137 DoD-1 — preserve canvas and latch | Met | Only protocol ID/action rendering changed; 0119 latch and 0125 layout tests pass. | None. |
| 0137 DoD-2 — correct preflight actions | Met | Review uses `<A>`; Set/protocol use plain `<a>`; `qc_gate` and unknown kinds have no dead link; protocol target exists. | None. |
| 0137 DoD-3 — recorded | Unmet | Completion governance pending. | Post-review gate. |

## Findings

### [P1] Terminal-only snapshots do not complete newly accepted actions

Confidence: High  
Requirement: 0133 DoD-2/DoD-3; 0134 DoD-2; 0136 DoD-2  
Location: [Process poller](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/process.rs:575), [extract drain](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/process.rs:637), [runner start](/C:/dev/Dedupe/crates/process-runner/src/runner.rs:523), [runner resume](/C:/dev/Dedupe/crates/process-runner/src/runner.rs:641)

Problem: Completion is recognized only when the previous UI snapshot was busy:

```text
finished_ok = was_busy && !snapshot_busy(new_snapshot)
```

Successful start/resume does not record the accepted job ID or update the local progress signal. The runner replies before executing the handler. If the job finishes before the first 400 ms poll, the UI observes idle/failed/succeeded → terminal without ever observing running.

Evidence:

- Queue removal and dispatch of the next PST occur only under `finished_ok`.
- The importing fallback only works when the cached page already contains a source with `status == "importing"`.
- Runner start replies at line 525 before `handler.run` at line 540.
- Resume replies at line 642 before its handler runs.
- The current tests exercise `sources_importing == true`, but not an accepted job transitioning directly to terminal.

Failure scenario:

- A small PST finishes extraction before the first poll. `Extract remaining` leaves the first entry in `extract_queue`, never dispatches the next PST, and subsequent clicks silently fail the queue-empty guard.
- An ingest reload races ahead of source insertion; the terminal snapshot is ignored and the new source never appears.
- A fast exception Retry leaves the job painted failed and Retry visible until manual refresh.

Correction: Track accepted start/resume job IDs. Treat a matching terminal snapshot as completion exactly once even if running was never polled. Advance extraction queues by accepted job ID rather than only `was_busy`, and refresh ingest/retry state on matching terminal snapshots.

Verification: Add deterministic idle→terminal tests for a two-entry extract queue, ingest before source insertion, fast unsupported ingest, and fast Retry.  
Deferrable: No

### [P2] `failed_unlogged` directs the operator to a nonexistent Resume action

Confidence: High  
Requirement: 0133 failed-unlogged recovery path  
Location: [footnote](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/process.rs:1432), [job actions](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/process.rs:1264), [orphan predicate](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/process.rs:396)

Problem: The footnote says “use job Resume,” but the Jobs table only displays Resume for a job whose durable state is `running` and whose snapshot looks orphaned. It never displays Resume for `failed` or `paused`.

Evidence: `failed_unlogged` specifically counts failed jobs without item errors. Such jobs cannot receive the exception-group Retry because no error group exists for them.

Failure scenario: A failed job has no `item_errors`. It increases Unaccounted-for and the UI tells the operator to use Resume, but neither the job row nor Exceptions offers that action.

Correction: Expose job-row Resume for failed/paused jobs using the existing `process_resume` error-surfacing path while preserving the exact orphan lock.

Verification: Render/action tests for failed, paused, succeeded, active-running, and orphan-running rows.  
Deferrable: No

### [P2] Non-Busy multi-file drop failures omit the unqueued filenames

Confidence: High  
Requirement: 0134 multi-file fail-closed policy  
Location: [drop error policy](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/process.rs:210), [drop invocation](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/process.rs:358)

Problem: When the first start fails with Busy, all dropped names are listed. For every other start error, `drop_error_after_start` returns only the original error and omits every unqueued path.

Failure scenario: Two files are dropped on an encrypted or otherwise rejected matter. Neither is queued, but the error does not list their basenames as required.

Correction: On every failed start, append all unqueued basenames. Preserve that list if the post-start page refresh also fails.

Verification: Add non-Busy failure tests, including encrypted/invalid-root errors with multiple paths.  
Deferrable: No

### [P2] Drop listener registration can fail silently

Confidence: High  
Requirement: 0134 DoD-2; no no-op or silent fallback  
Location: [listener promise handling](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/process.rs:249), [listener setup](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/process.rs:755), [host emit](/C:/dev/Dedupe/crates/dedupe-chrome/src/lib.rs:620)

Problem:

- A rejected direct `onDragDropEvent` promise is discarded rather than triggering the host-event branch.
- A rejected `event.listen` promise is discarded.
- If both synchronous probes fail, no error is surfaced.
- Host `emit` errors are ignored.

Tauri documents `onDragDropEvent` and `listen` as fallible promises returning an unlisten function; the selected API shape itself is correct. [Tauri webview API](https://github.com/tauri-apps/tauri/blob/tauri-v2.11.5/packages/api/src/webview.ts#L609-L663) Current `core:default` also includes the required event permissions. [Tauri core permissions](https://v2.tauri.app/reference/acl/core-permissions/)

Failure scenario: An initialization, ACL, IPC, or namespace timing failure leaves the drop zone inert without an error or fallback, while the UI continues presenting it as operational.

Correction: Await registration results; on direct rejection try the host listener; surface a terminal error if both fail. Handle cleanup that occurs before the registration promise resolves.

Verification: Mock resolved/rejected listener promises and run the required release-EXE drop HITL.  
Deferrable: No

## Completeness Sweep

- No new TODO, FIXME, stub, placeholder, mock-data, ignored-test, or fake-count implementation was found.
- No schema migration, `jobs.params_json`, OST/MBOX copy, password vault, QC-queue route, auto-extract, auto-profile, or app.css change was introduced.
- Host and WASM DTOs agree; every new WASM field has `#[serde(default)]`.
- Source sizing is file-only and does not follow reparse points or walk directories.
- `unaccounted_for` arithmetic is unchanged.
- The required production locks each occur exactly once:
  - `extract_queue.set(Vec::new())`
  - `is_orphan_running(&job_for_orphan, &progress.get())`
  - `{j.kind.clone()}`
- The principal test weakness is reliance on source-string assertions for asynchronous behavior. Those tests pass while Findings 1, 3, and 4 remain possible.

## Wiring and Regression Review

- Reconciliation: matter inventory/checkpoints → host response → WASM DTO → named Process list is wired.
- Extraction: CTA → `process_start` → runner → extract handler is reachable, but terminal-only completion breaks the UI drain.
- Drop: Tauri drag event → direct callback or host event → `process_start(ingest)` is wired, subject to Finding 4.
- Exceptions: real `item_errors` → grouped DTO → conditional Retry/Open in review is wired, subject to fast completion refresh.
- Produce: live extras → review link or in-page target is correctly wired.
- Schema 41, encrypted-matter rejection, source read-only behavior, 0119 latch, 0125 canvas, 0122 queue/orphan locks, and 0126 arithmetic remain intact.
- D-0034-06 and code-signing work remain outside this branch; no signing or secret boundary was changed.

## Verification Evidence

Observed now:

- `cargo fmt --all --check` — passed.
- Current UI test executable — 53 passed, 0 failed, 0 ignored.
- Six targeted host pure tests — passed, including arithmetic, schema, path labels, extract label mapping, and independent exception samples.
- `git diff --check` — passed.
- Ledgerful impact scan — high-risk working tree, chiefly due Process/Produce UI coupling.

Reported by the implementer:

- `cargo test -p dedupe-chrome --lib` — 129 passed.
- UI tests — 53 passed.
- `cargo fmt --all`.

Not verifiable in this read-only sandbox:

- Cargo could not acquire `target\debug\.cargo-lock`.
- Direct execution of filesystem-dependent host tests was blocked because temporary directories could not be created; this was an environment failure, not a code failure.
- Ledger pending/signature state could not be opened.
- Production WASM/Tauri build and release-EXE drag/drop HITL.
- Workspace clippy was neither reported nor observable.

Required before completion:

- Fix all four findings and re-review.
- Run host/UI tests, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`.
- Build the production WASM/Tauri surface.
- Run the release-EXE synthetic drop and fast-job HITL.
- Verify Ledgerful pending/signature state.

## Deferred Candidates

None. All findings are P1/P2 and are not deferrable.

## Completion Decision

Do not mark tracks 0133–0137 Completed. After the findings are fixed and gates pass, write the canonical review, reconcile all Ready/In-progress governance entries to Completed, close only the validated deferred rows, and commit the required Ledgerful FEATURE provenance.