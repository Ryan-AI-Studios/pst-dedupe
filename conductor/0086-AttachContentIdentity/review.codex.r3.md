# Track Completion Audit — 0086-AttachContentIdentity

## Verdict: FAIL

## Scope Reviewed

Read-only review of the working tree against `origin/main`, including `spec.md`, `plan.md`, implementation, tests, docs, prior reviews, and untracked files.

## Requirement and DoD Matrix

| Item | Result | Evidence |
|---|---|---|
| Prior partial-enumeration P1 | Fixed | Strict row-error propagation and fail-closed scan classification |
| CLI surfaces and parsing | Met | Shared parser and flags across required surfaces |
| Streaming SHA-256 / Choice B | Met on normal path | 64 KiB streaming helper, sentinels, length checks |
| No omitted identity slots | Unmet | `--no-attachments` bypass |
| Grouping split/refinement tests | Met | Synthetic PST and hasher tests |
| Stats/truncation propagation | Met | Scan, summary, human output, rebuild preservation |
| Docs/deferred records | Met | Live attach-content docs; D-0076 closed; D-0086 residuals recorded |
| Gates | Reported pass | Orchestrator supplied fmt, workspace tests, targeted clippy |
| DoD-9 governance | Pending process work | Board remains In Progress; canonical review absent |

## Findings

### [P1] `--no-attachments` silently disables `body-recip-attach`

Confidence: High  
Requirement: DoD-2, DoD-4; Choice B no-omit invariant; “always walk attaches when `need_attach_content`.”  
Location: [scan.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:793), [main.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/main.rs:1694), [keep_set_cmd.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/keep_set_cmd.rs:189), [unique_eml_cmd.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_eml_cmd.rs:187), [unique_pst_cmd.rs](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:1404)

Problem: `walk_attaches` requires `opts.include_attachments`. With `--no-attachments`, the scan produces `Vec::new()` without enumeration, failure classification, or warning. `body-recip-attach` then hashes zero attachment slots.

Failure scenario: Two messages with identical body/recipients and same name/size but different attachment bytes collapse when run with `--strong-content-hash body-recip-attach --no-attachments`.

Correction: Reject this flag combination, or decouple identity enumeration/digesting from output attachment inclusion. Add an integration regression test.

Verification: Not currently covered by the supplied tests.

Deferrable: No

## Prior-Finding Reverification

The reported partial-enumeration fix is present:

- `list_attachments_strict` propagates corrupt row/property errors.
- `scan` uses strict enumeration for attach-content identity.
- `has_attachments=true` with an empty strict result skips the message.
- Truncation counters are propagated through summaries and rebuilds.
- Product documentation no longer contains the stale “not live” claim.

## Completeness Sweep

No new production placeholders, fake digests, `read_to_end`, or multi-GB attach buffers were found. The untracked `fixtures/keep_set_summary.json` appears to be generated output outside `output/`; it was not used as verification evidence.

## Verification Evidence

- `git diff --check`: passed.
- Orchestrator-reported fmt, workspace tests, and targeted clippy: passed.
- Ledgerful status/impact: unavailable in this read-only environment due database/report write permission errors.
- No files or Git state were modified by this review.

## Deferred Candidates

After the blocking issue is fixed, the documented `D-0086-embedded-email-hash`, `D-0086-digest-probe-unify`, and process DoD-9 closeout remain valid deferred/process items.

## Completion Decision

FAIL.