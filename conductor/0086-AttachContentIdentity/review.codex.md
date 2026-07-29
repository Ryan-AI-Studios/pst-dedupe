# Track Completion Audit — 0086-AttachContentIdentity

## Verdict: FAIL

## Scope Reviewed

Read-only review of the uncommitted working tree against `origin/main`, including:

- `spec.md` and `plan.md`
- Changed CLI, dedup-engine, GUI, documentation, governance, and test files
- `attach_content_hash.rs` and `attach_content_0086.rs`
- Internal reviews r1/r2
- Cached Ledgerful impact and verification reports
- Track board/status and deferred items

No files or Git/Ledgerful state were modified.

## Requirement and DoD Matrix

| Requirement / DoD | Result | Assessment |
|---|---|---|
| CLI enablement on all five surfaces | Met | Flags, parsing, help, wiring, and warning paths exist. |
| Choice B sentinel formula | Met | Domain separation, lowercase name, size encoding, and sentinel tests are present. |
| Full-stream hashing | Partial | Normal, cloud, open/read failure, CRC, cancellation, budget, and mismatch paths are implemented. Attachment-table enumeration failure can still erase all slots. |
| No omitted/tier-downgraded attachments | Failed | BestEffort metadata enumeration failure falls back to an empty attachment list. |
| Budgets and dedicated flags | Partial | Flags and enforcement are wired, but truncation state is not exposed in final stats/reports. |
| Synthetic split fixture | Met | Different payloads split at `body-recip-attach` while matching at `body-recip`. |
| Hijack, empty/mismatch, cloud tests | Partial | Unit/integration coverage exists, but no test covers attachment-table enumeration failure. |
| NIST multi-block KAT | Met | KAT and chunked consistency tests are present. |
| Refinement and default-off behavior | Met | Strong identity refines body-recip; lower levels do not stream attachment content. |
| Keep-set strong-hash reuse | Met by inspection | Stored `strong_content_hash` is reused during rebuild; no re-digest is performed. |
| Documentation and deferred records | Partial | Main documentation is present, but one stale “not live” claim contradicts the implementation. |
| Verification and governance DoD | Unmet / not verifiable | Cached verification predates the latest `scan.rs` change; no current Ledgerful transaction was observed; `review.md` is absent and the board remains In Progress. |

## Findings

### [P1] Attachment metadata enumeration can violate Choice B and permit false identity matches

- **Confidence:** High
- **Requirement:** Choice B no-omit/no-downgrade invariant; DoD-2 and DoD-4.
- **Location:** `crates/pst-dedup-cli/src/scan.rs:798-855`; `crates/dedup-engine/src/integrity.rs:741-758`
- **Problem:** When `list_attachments` fails in BestEffort mode, the scan converts the failure into `Vec::new()`. Strong hashing then computes the attachment portion with zero slots.
- **Impact:** An attachment-bearing message can receive the same `body-recip-attach` identity as an otherwise equivalent message with no attachments. This is precisely the prohibited omission/hijack case.
- **Correction:** Attachment-content identity must fail closed when attachment enumeration is unavailable; it must not proceed with an empty attachment list or silently downgrade.
- **Deferrable:** No.

### [P2] Budget and cancellation truncation is not represented in final statistics

- **Confidence:** High
- **Requirement:** Budget reporting and honest unread/truncation statistics.
- **Location:** `crates/pst-dedup-cli/src/attach_content_hash.rs:66-73`; scan summary/stat propagation.
- **Problem:** `AttachContentHashState` tracks `truncated`, but that value is never propagated into `GroupingStats`, `ScanSummary`, or output reports. Consumers can see unread counts but cannot distinguish ordinary unread content from budget/cancellation truncation.
- **Correction:** Propagate and preserve a truncation indicator through scan summaries, rebuilds, and relevant reports.
- **Deferrable:** No.

### [P2] Documentation contains a stale contradictory claim

- **Confidence:** High
- **Requirement:** Documentation and governance agreement; DoD-7.
- **Location:** `docs/unique-pst-export.md:507`
- **Problem:** The document still states that attach-payload identity is “not live,” contradicting the new `body-recip-attach` documentation and implementation.
- **Correction:** Remove or update the stale D-0076 claim to describe the shipped 0086 behavior.
- **Deferrable:** No.

### [P2] Track completion governance is not closed

- **Confidence:** High
- **Requirement:** DoD-8 and DoD-9.
- **Evidence:** `conductor/conductor.md` still marks 0086 In Progress; `conductor/0086-AttachContentIdentity/review.md` is absent; no 0086 Ledgerful FEATURE commit was observed.
- **Additional verification issue:** `ledgerful ledger status --compact` failed with `unable to open database file`.
- **Correction:** Complete the canonical review/governance closeout, update the board, record the Ledgerful transaction, and rerun verification against the final tree.
- **Deferrable:** No.

## Completeness Sweep

- No new production placeholders, stubs, fake digests, no-op paths, or silent fallback values were found beyond the attachment-list failure described above.
- Real attachment hashing uses fixed 64 KiB streaming buffers; no multi-GB `Vec` or `read_to_end` path was found.
- Empty attachments and declared-size mismatches have explicit handling.
- Cloud/unread attachments receive deterministic Choice B sentinels.
- Inline-ignore behavior is soft-warning based and omits both metadata and content slots.
- Embedded-message content hashing is implemented as a raw stream; recursive handling remains explicitly deferred.
- The critical missing coverage is attachment metadata enumeration failure and externally visible truncation reporting.

## Wiring and Regression Review

The end-to-end wiring is present for scan, dups, keep-set, unique-eml, and unique-pst. Dedicated budget flags reach the scan worker. Decision records use identity version `v2` and `content_hash_strong` provenance.

Keep-set rebuilds reuse stored strong hashes through `RecoverableScanItem`; the code does not re-read or rehash attachment content during grouping. GUI behavior remains body-only.

The synthetic integration test proves the primary split behavior, but regression coverage does not protect the failing metadata-enumeration path or verify truncation reporting.

## Verification Evidence

- Cached `latest-verify.json` reports fmt, clippy, and workspace tests exit 0.
- That artifact is stale: its timestamp predates the latest `scan.rs` modification, so it is not evidence for the resulting working tree.
- `cargo fmt`, clippy, and workspace tests were not rerun because this review was explicitly read-only.
- `cargo deny` was not independently observed.
- `git diff --check` fails on trailing whitespace in `CHANGELOG.md:14`.
- Ledgerful status was unavailable due to the database-open failure.

## Deferred Candidates

These existing P3 residuals are reasonable deferred candidates after the blocking findings are corrected:

- `D-0086-embedded-email-hash`
- `D-0086-digest-probe-unify`

They are explicitly documented, non-blocking, and do not excuse the Choice B failure, missing truncation reporting, stale documentation, or incomplete governance closeout.

## Completion Decision

FAIL.

The primary implementation is substantially present and the core hashing tests are strong, but the BestEffort attachment-enumeration fallback violates the central Choice B identity invariant. Truncation reporting, documentation consistency, current-tree verification, and completion governance also remain unresolved.