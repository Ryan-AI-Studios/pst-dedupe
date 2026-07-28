# Track Completion Audit — 0077-CrcNoiseAndExportRisk

## Verdict: FAIL

The claimed fixes close the four prior P2 implementation defects, but two P1 correctness gaps remain. Completion governance is also unfinished. PASS WITH DEFERRED P3 is not permitted.

## Scope Reviewed

Read-only review of:

- Track `spec.md`, `plan.md`, implementation notes, prior reviews.
- Working tree on `feat/0077-crc-noise-export-risk`.
- CRC telemetry, scan attribution, taint, grouping, export-risk, writer, GUI, tests, and documentation.
- Cached Ledgerful impact report.

No files or Git state were modified.

## Prior Finding Verification

| Prior finding | Result | Evidence |
|---|---|---|
| P1 final attachment stream CRC | Partially fixed | `AttachRead` shared flag and adapter exist at [production.rs:210](/C:/dev/Dedupe/crates/pst-writer/src/production.rs:210), [unique_pst_cmd.rs:517](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:517), and [unique_pst_cmd.rs:573](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:573). The writer emits `ATTACH_STREAM_CRC` after successful consumption at [production.rs:2639](/C:/dev/Dedupe/crates/pst-writer/src/production.rs:2639)–[production.rs:2692](/C:/dev/Dedupe/crates/pst-writer/src/production.rs:2692), with Info severity at [production.rs:623](/C:/dev/Dedupe/crates/pst-writer/src/production.rs:623). The writer test is present at [writer_streaming.rs:700](/C:/dev/Dedupe/crates/pst-writer/tests/writer_streaming.rs:700). However, the late stream hit is not added to `export_risk` or message integrity; see Finding P1-1. |
| P1 poly strip / rule 10 | Not fixed | `CRC_SUSPECT` remains stored, but dual-rate sources automatically regain Tier-2 eligibility through [grouping.rs:224](/C:/dev/Dedupe/crates/dedup-engine/src/grouping.rs:224)–[grouping.rs:275](/C:/dev/Dedupe/crates/dedup-engine/src/grouping.rs:275) and [scan.rs:1084](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1084)–[scan.rs:1108](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1108). This still permits identity computation from known suspect bytes, contrary to the locked rule at [spec.md:80](/C:/dev/Dedupe/conductor/0077-CrcNoiseAndExportRisk/spec.md:80) and DoD-20 at [spec.md:348](/C:/dev/Dedupe/conductor/0077-CrcNoiseAndExportRisk/spec.md:348). |
| P2 source-local `distinct_bad_bids` | Fixed | Source-local set lifecycle at [integrity_telemetry.rs:207](/C:/dev/Dedupe/crates/pst-reader/src/integrity_telemetry.rs:207)–[integrity_telemetry.rs:249](/C:/dev/Dedupe/crates/pst-reader/src/integrity_telemetry.rs:249); scan calls `begin_source` per file at [scan.rs:543](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:543); regression test at [integrity_telemetry.rs:605](/C:/dev/Dedupe/crates/pst-reader/src/integrity_telemetry.rs:605). |
| P2 degraded accounting | Fixed | Degraded counts now retain `CRC_SUSPECT` normally at [scan.rs:826](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:826)–[scan.rs:861](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:861); the former strip/removal decrement is gone. |
| P2 BID fixture | Fixed | Trailer BID is flipped at [crc_integrity_0077.rs:64](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/crc_integrity_0077.rs:64)–[crc_integrity_0077.rs:90](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/crc_integrity_0077.rs:90), and `block_bid_mismatches > 0` is asserted at [crc_integrity_0077.rs:158](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/crc_integrity_0077.rs:158)–[crc_integrity_0077.rs:171](/C:/dev/Dedupe/crates/pst-dedup-cli/tests/crc_integrity_0077.rs:171). |
| P2 scan JSON deserialization | Fixed | `FileScanStats` and `ScanSummary` derive `Deserialize` at [scan.rs:98](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:98) and [scan.rs:137](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:137); compatibility test at [scan.rs:1737](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1737)–[scan.rs:1805](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1805). |

## Findings

### [P1] Late final-export CRC is reported only as an event, not export risk

Confidence: High

Requirement: DoD-19, DoD-22, post-export `export_risk`.

Location: [unique_pst_cmd.rs:2165](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:2165)–[unique_pst_cmd.rs:2183](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:2183); [unique_export_report.rs:170](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_export_report.rs:170)–[unique_export_report.rs:180](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_export_report.rs:180).

Problem: A CRC discovered while the final attachment stream is consumed emits `ATTACH_STREAM_CRC`, but it does not update message integrity, `degraded_winner_rate`, `block_crc_rate`, or `export_risk`.

Failure scenario: Scan/deep-probe does not read an attachment, final PST writing encounters a warning-only CRC, and suspect bytes are successfully exported while `summary.json` still reports the pre-export risk.

Correction: Carry final stream CRC evidence into the post-export risk inputs/report, or guarantee that final attachment reads are taint-connected before winner resolution.

Verification: Existing writer event test passes as claimed; no test proves final stream CRC changes risk or final integrity reporting.

Deferrable: No.

### [P1] Dual-rate poly auto-allow still violates the locked identity rule

Confidence: High

Requirement: Locked rule 10, DoD-12, DoD-20.

Location: [grouping.rs:224](/C:/dev/Dedupe/crates/dedup-engine/src/grouping.rs:224)–[grouping.rs:275](/C:/dev/Dedupe/crates/dedup-engine/src/grouping.rs:275); [scan.rs:1084](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1084)–[scan.rs:1108](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1108).

Problem: The fix removes the visible reason strip but automatically enables Tier-2 for sources classified as “poly.” That still allows `content_hash` identity to be computed from bytes marked `CRC_SUSPECT`. Retaining the taint in metadata does not prevent the unsafe identity operation.

Failure scenario: A genuinely corrupt source produces high page and block CRC rates and is classified as poly; suspect and clean messages can then merge through Tier-2 without an explicit operator override.

Correction: Keep automatic poly classification observational only, or obtain explicit specification authority for this exception with a real polynomial fingerprint/allowlist. Automatic Tier-2 restoration cannot satisfy the current rule.

Verification: The dual-rate unit test proves classification, not that suspect identities remain blocked.

Deferrable: No.

### [P2] Completion gate and residual severity are not satisfied

Confidence: High

Requirement: DoD-17 and DoD-18; user-specified P3-only residual policy.

Evidence:

- No canonical `conductor\0077-CrcNoiseAndExportRisk\review.md` exists.
- `conductor.md` still says **In Progress** at [conductor.md:178](/C:/dev/Dedupe/conductor/conductor.md:178).
- `sequencing.md` still says **Ready** at [sequencing.md:144](/C:/dev/Dedupe/conductor/sequencing.md:144).
- `docs/deferred.md` records `D-0077-parallel-attrib` as **P2** at [deferred.md:810](/C:/dev/Dedupe/docs/deferred.md:810).
- `ledgerful ledger status --compact` failed with `unable to open database file`; no successful `ledgerful verify` result was observed.
- The cached impact report is high risk and reports `treeClean: false`.

Correction: Resolve or formally re-scope the P2 residual, complete the required canonical review/governance artifacts, and obtain successful Ledgerful verification.

Deferrable: No.

## Requirement and DoD Matrix

| DoD | Result | Evidence |
|---|---|---|
| 1 | Met | Telemetry module, TLS/global counters, BID cap, snapshot/delta/reset/config APIs. |
| 2 | Met | Page/block CRC sites route through telemetry at [page.rs:107](/C:/dev/Dedupe/crates/pst-reader/src/ndb/page.rs:107) and [block.rs:78](/C:/dev/Dedupe/crates/pst-reader/src/ndb/block.rs:78). |
| 3 | Met | Bounded emission test at [integrity_telemetry.rs:658](/C:/dev/Dedupe/crates/pst-reader/src/integrity_telemetry.rs:658). |
| 4 | Met | Serde defaults and legacy deserialization test. |
| 5 | Met | Rates and read denominators are computed at [scan.rs:1507](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1507)–[scan.rs:1517](/C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1517). |
| 6 | Met | Existing `PreflightRecommendation` used by `ExportRisk`. |
| 7 | Met | Monotone max composition at [unique_export_report.rs:328](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_export_report.rs:328). |
| 8 | Met | Advisory/catastrophic threshold branches and tests present. |
| 9 | Met | Flags present on the five CLI surfaces. |
| 10 | Met | Page, block, and BID fixture classes asserted. |
| 11 | Met | Event cap and exact/truncated fields at [production.rs:692](/C:/dev/Dedupe/crates/pst-writer/src/production.rs:692)–[production.rs:749](/C:/dev/Dedupe/crates/pst-writer/src/production.rs:749). |
| 12 | Unmet | Poly auto-allow permits suspect Tier-2 identity; see P1. |
| 13 | Partial | Scan line is counters/codes-only; unique-pst human output still exposes only `export_risk`, documented as residual. |
| 14 | Met | ScanPST/Purview decision tree at [unique-pst-export.md:201](/C:/dev/Dedupe/docs/unique-pst-export.md:201)–[unique-pst-export.md:231](/C:/dev/Dedupe/docs/unique-pst-export.md:231). |
| 15 | Met | SEC-06 updated at [audit.md:341](/C:/dev/Dedupe/docs/audit.md:341). |
| 16 | Partial | Fixture-scale timing only; comparable/multi-GB proof remains absent. |
| 17 | Not verifiable | Cargo gates are reported green by the orchestrator; Ledgerful verification was not observed and status failed. |
| 18 | Unmet | Canonical review and Completed statuses are absent. |
| 19 | Partial | Body/metadata/attach-probe taint is wired; final export stream only emits an event and does not affect risk. |
| 20 | Unmet | Ordinary suspect items are blocked, but poly sources automatically restore Tier-2. |
| 21 | Met | Explicit `CrcSuspect` fidelity mapping at [keepset.rs:1302](/C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:1302)–[keepset.rs:1316](/C:/dev/Dedupe/crates/dedup-engine/src/keepset.rs:1316). |
| 22 | Partial | Per-source/total JSON and scan human output exist; final stream CRC is not reflected in the risk/report counters. |
| 23 | Met | GUI risk mapping at [unique_worker.rs:112](/C:/dev/Dedupe/crates/pst-dedup-gui/src/unique_worker.rs:112)–[unique_worker.rs:125](/C:/dev/Dedupe/crates/pst-dedup-gui/src/unique_worker.rs:125), rendered at [unique_wizard.rs:366](/C:/dev/Dedupe/crates/pst-dedup-gui/src/views/unique_wizard.rs:366)–[unique_wizard.rs:430](/C:/dev/Dedupe/crates/pst-dedup-gui/src/views/unique_wizard.rs:430). |

## Completeness Sweep

No new production stubs or fake CRC counters were found in the reviewed paths. Core telemetry, writer event capping, GUI risk banners, JSON defaults, and fixture assertions are wired.

The meaningful fresh regression is that the poly workaround still changes Tier-2 identity semantics, and late writer-stream CRC evidence stops at an Info event.

## Verification Evidence

Observed now:

- Correct branch and dirty working tree.
- `git diff --check`: passed.
- Cached Ledgerful impact report: high risk, `treeClean: false`.
- `ledgerful ledger status --compact`: failed with `unable to open database file`.

Reported by the orchestrator, not independently rerun:

- `cargo fmt --all --check`: green.
- `cargo clippy --workspace --all-targets -- -D warnings`: green.
- `cargo test --workspace`: green.

## Deferred Candidates

Qualifying P3 residuals include:

- DoD-16 comparable multi-GB performance proof.
- Optional tracing Layer, richer Desk drill-down, and ScanPST count-diff wrapper.
- Unique-pst human summary detail polish.

`D-0077-parallel-attrib` is currently recorded as P2 and cannot be accepted under a P3-only completion policy.

## Completion Decision

FAIL. The four prior P2 implementation defects are fixed with evidence, but the poly identity exception and late final-export CRC risk gap remain P1. The track also lacks required completion artifacts and successful Ledgerful verification.