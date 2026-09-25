# Track Completion Audit — 0140-ScanStderrCadence

## Verdict: FAIL

The implementation wiring is sound, but two required completion gates remain unmet.

| Requirement / DoD | Result | Evidence |
|---|---|---|
| Folder cadence, per-file timer, `-vv` behavior | Met | `scan.rs` initializes per file and re-arms after INFO; non-cadence folders use DEBUG. |
| Probe gating and captured `first_n` | Met | Both scan and unique-pst callbacks use the shared helper with a value captured outside the closure. |
| Unique-pst end summary retained | Met | Summary remains outside `progress_cb` gating. |
| Getter, clap help, docs, JSON isolation | Met | Getter test, five help surfaces, CHANGELOG/docs, and JSON parsing tests are present. |
| DoD-1 CLI proof | Partial | The test exists, but its classifier violates the locked test contract. |
| DoD-6 recorded/finalized | Unmet | No `review.md`; registry remains In progress/Ready; ledger FEATURE state is not verifiable. |

## Findings

[P1] DoD-6 completion record is absent  
Confidence: High  
Requirement: DoD-6  
Location: `conductor/0140-ScanStderrCadence/review.md` (missing); `conductor/conductor.md:370`  
Problem: The canonical review file does not exist, and governance does not mark 0140 Completed. The branch has no commits beyond `origin/main`; all work is in the dirty working tree. Ledger status could not be read because Ledgerful’s database could not be opened, so the required FEATURE ledger record is not verifiable.  
Correction: Complete the required closeout after resolving the test finding: create canonical review evidence, verify the FEATURE ledger record, and update the registry consistently to Completed.  
Deferrable: No  

[P2] Cadence integration test counts every “scan progress” event instead of folder-only events  
Confidence: High  
Requirement: Spec §2.6 and DoD-1; plan Phase 3 explicitly requires `folder=` without `msg_i=`.  
Location: [scan_integrity.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/scan_integrity.rs:361)  
Problem: `folder_progress_line_count` uses `stderr.matches("scan progress")`. Message-every-500 progress intentionally uses the same event message, so the test does not implement the required folder-only classifier.  
Failure scenario: A fixture or scan with 500+ messages adds legitimate `msg_i=` events and can make the `-v`/`-vv` assertions fail despite correct folder cadence—or obscure an incorrect folder/message field split.  
Correction: Count events containing `folder=` and not `msg_i=` (with ANSI-safe handling if needed).  
Deferrable: No  

## Verification Evidence

- Observed: `git diff --check origin/main` passed.
- Reported by requester, not rerun in this read-only audit: fmt, clippy, workspace tests, and `ledgerful verify` passed.
- Observed: `ledgerful ledger status --compact` failed locally with a rusqlite database-open error; no ledger conclusion was inferred.
- No track-local placeholder, no `--progress-file`, and no unique-pst `stage=` rewrite found.

After the test correction and DoD-6 closeout, this should be re-reviewed.