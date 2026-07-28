# Track Completion Audit — 0077-CrcNoiseAndExportRisk

## Verdict: FAIL

## Scope Reviewed

Read all of `spec.md` and `plan.md`, the uncommitted working-tree implementation, tests, documentation, deferred rows, prior review artifacts, and completion metadata. No files or Git state were modified.

## Requirement and DoD Matrix

| Area | Status | Evidence |
|---|---|---|
| Data-path telemetry and CRC warning routing | Met | TLS counters, global flush, bounded emission, all CRC warning sites routed |
| Per-source counters and rates | Partial | Counters exist, but `distinct_bad_bids` is global post-state, not source-local |
| `crc_skip_rate` compatibility | Met | Message-skip semantics remain separate; fixture test pins zero skips |
| `CRC_SUSPECT` message/body/metadata wiring | Met | Message properties, extracts, metadata probes, and attach probes are wired |
| Final attachment export stream taint | Unmet | Stream reader is erased to `Box<dyn Read>` before writer consumption; late CRC taint is lost |
| Tier-2 default ineligibility | Partial | Correct for ordinary corruption, but poly stripping restores identity eligibility for known CRC failures |
| Export risk vocabulary and monotonicity | Met | Uses existing `PreflightRecommendation`; no second enum found |
| Attachment event cap | Met | Cap 1000, total/truncated fields, success-path report wiring |
| CLI flags and exit behavior | Met | Five subcommands wired; no exit-policy change |
| JSON/human reporting | Partial | Scan reporting exists; unique-pst human output omits CRC counters |
| Documentation and SEC-06 | Met | Decision tree, SEC-06 update, deferred rows present |
| Completion governance | Unmet | No track `review.md`; conductor remains In Progress and sequencing remains Ready |

### DoD Matrix

| DoD | Status |
|---|---|
| 1 | Met |
| 2 | Met |
| 3 | Met |
| 4 | Partial — no `Deserialize` implementation for `FileScanStats`/`ScanSummary`; source-local distinct BID attribution is incorrect |
| 5 | Met |
| 6 | Met |
| 7 | Met |
| 8 | Met |
| 9 | Met |
| 10 | Partial/Unmet — page and block CRCs are asserted, but BID mismatch is neither correctly generated nor asserted |
| 11 | Met |
| 12 | Unmet — poly stripping conflicts with locked rule 10, and split-only regression proof is incomplete |
| 13 | Met for the lines reviewed |
| 14 | Met |
| 15 | Met |
| 16 | Partial — fixture-scale timing only; no comparable before/after or multi-GB evidence |
| 17 | Unmet/Unverified — Ledgerful status, impact scan, and verify could not run; full gate evidence was reported but not independently rerun |
| 18 | Unmet — no `review.md`; conductor/sequencing statuses not completed |
| 19 | Partial — final writer attachment stream is not taint-connected |
| 20 | Partial — default behavior is correct except for automatic poly identity stripping |
| 21 | Met |
| 22 | Partial — scan human summary reports CRC data; unique-pst human summary reports only `export_risk` |
| 23 | Met |

## Findings

### [P1] Final streaming attachment reads lose CRC taint

Locations: [`pst_materializer.rs`]( /C:/dev/Dedupe/crates/pst-dedup-cli/src/pst_materializer.rs:523), [`production.rs`]( /C:/dev/Dedupe/crates/pst-writer/src/production.rs:2608), [`unique_pst_cmd.rs`]( /C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_pst_cmd.rs:2136).

`AttachmentDataReader` tracks `crc_suspect`, but `PstAttachStreamSource::open_attach` returns it as `Box<dyn Read>`. The writer consumes that erased reader and maps failures only to `ATTACH_STREAM_READ_FAILED`; a warning-only CRC mismatch returns successful bytes without emitting `CRC_SUSPECT`.

`export_risk` is then computed from the pre-export scan summary, with no post-stream telemetry delta. A CRC failure discovered while writing a final attachment can therefore produce suspect output without message taint, attachment fidelity evidence, or updated export risk.

This is a core DoD-19/rule-10 wiring failure and is not deferrable.

### [P1] Dual-rate poly stripping still violates locked rule 10

Locations: [`scan.rs`]( /C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1074), [`scan.rs`]( /C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1615).

When both page and block CRC rates are at least 0.50, the implementation removes `CRC_SUSPECT` from candidates and rows. This is a heuristic, not proof that the CRC polynomial is wrong. The bytes were still read after a known block CRC/BID failure, and identity may then be computed from them.

The deferred row calls this a P3 “policy,” but the current spec’s locked rule 10 explicitly says corruption recovered from remains corruption and suspect bytes must never compute identity. The deferred note does not amend that rule. This must be resolved by explicit product/spec authority or by preserving taint.

### [P2] `distinct_bad_bids` is not actually per-source

Locations: [`integrity_telemetry.rs`]( /C:/dev/Dedupe/crates/pst-reader/src/integrity_telemetry.rs:65), [`scan.rs`]( /C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:122).

`delta_since` returns the global distinct-BID set cardinality whenever the source delta has any mismatch. With two sources, source B reports the total distinct BID count accumulated from source A plus B. The field comment itself acknowledges “capped globally.”

This makes per-file JSON and per-source reporting incorrect even in sequential scans; it is not only a future parallel-attribution limitation.

### [P2] Post-strip accounting can disagree with integrity rows and candidates

Location: [`scan.rs`]( /C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:1134).

The implementation removes the number of `CRC_SUSPECT` reasons from `file_degraded` and `total_degraded`, even when the same message retains another degradation reason. In that case, the candidate remains degraded and `integrity.csv` retains the other reason, while summary degraded-message counts are decremented.

Additionally, buffered rows are matched by `pst_name` rather than full source path, so same-basename PSTs can have taint stripped from the wrong source.

### [P2] The synthetic fixture does not prove BID mismatch

Location: [`crc_integrity_0077.rs`]( /C:/dev/Dedupe/crates/pst-dedup-cli/tests/crc_integrity_0077.rs:60).

The fixture flips byte 511 of page trailers, not the BID field in a block trailer. The test asserts page CRC and block CRC counts, but never asserts `block_bid_mismatches > 0`. Therefore DoD-10’s deterministic three-class fixture requirement is incomplete.

### [P2] Additive JSON compatibility is not implemented for scan structs

Location: [`scan.rs`]( /C:/dev/Dedupe/crates/pst-dedup-cli/src/scan.rs:98).

`FileScanStats` and `ScanSummary` derive `Serialize` only. The new fields have `#[serde(default)]`, but these types cannot deserialize either old or new JSON. This does not satisfy DoD-4’s explicit “pre-0077 JSON still deserializes” requirement.

### [P2] Required completion artifacts are absent

The track has no `review.md`. `conductor/conductor.md` still marks 0077 **In Progress**, and `conductor/sequencing.md` still marks it **Ready**. `implementation-notes.md` explicitly says these steps were left to the orchestrator.

DoD-18 is therefore not complete.

## Completeness Sweep

No obvious production placeholders or fake CRC values were found. CRC remains warning-only, `crc_skip_rate` remains unchanged, the risk vocabulary is unified, and no exit-code changes were found.

The unresolved items above are functional or explicit DoD gaps, not merely polish.

## Wiring and Regression Review

- CRC counters are correctly wired through reader data paths and global summaries.
- Ordinary body corruption taints message candidates and blocks Tier-2 by default.
- Tier-1 MID grouping remains available.
- Attach probe/materialization paths consume `crc_suspect`.
- The final writer stream path does not.
- `integrity.csv` is buffered post-strip, but summary reconciliation is not message-aware.
- Poly stripping can re-enable identity on known suspect bytes.
- No two-source attribution test, final export-stream CRC test, proper BID fixture assertion, or split-only regression test was found.
- `export_risk` does not alter process exit behavior.

## Verification Evidence

Observed:

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- Working tree remains uncommitted as specified.

Reported by the orchestrator, not independently rerun in this read-only audit:

- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.

Ledgerful checks attempted but unavailable:

- `ledgerful ledger status --compact`: failed with `unable to open database file`.
- `ledgerful scan --impact`: failed writing `.ledgerful/reports/latest-scan.json`.
- `ledgerful verify`: no successful evidence available.

## Deferred Candidates

Only the following is suitable as a non-blocking P3:

- DoD-16 comparable multi-GB performance measurement and before/after overhead proof.

The poly identity strip, final attachment taint, per-source attribution, fixture coverage, JSON compatibility, and completion artifacts are not acceptable P3 deferrals.

## Completion Decision

**FAIL.**

The track cannot be completed until the P1/P2 findings are resolved, DoD-10/12/19 are re-proven with regression tests, Ledgerful verification is available, and the required `review.md` and Completed conductor status are added.