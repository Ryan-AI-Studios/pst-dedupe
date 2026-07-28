# Track Completion Audit - 0075-KeepSetWinnerPolicies

## Verdict: FAIL

## Scope Reviewed

Read the complete `spec.md` and `plan.md`, reviewed the working-tree implementation against `origin/main`, and audited the engine, reader, CLI, GUI, tests, docs, governance files, and verification state.

No files or Git state were modified.

## Requirement and DoD Matrix

| Area | Result |
|---|---|
| RankContext and default-off ladder | Mostly implemented; default reader compatibility defect remains |
| Earliest-date resolution | Submit → delivery → missing-last implemented; pre-1970 serialization defect |
| BCC preference/statistics | Implemented and wired |
| Folder-class ladder/globs | Partial; overlapping folder segments can select the wrong class |
| Source-rank/inversion | Implemented and wired |
| `decided_by` and appended CSV fields | Implemented append-only |
| All Custodians aggregate | Implemented; three-surface parity is insufficiently tested |
| Graded fidelity | Implemented; mapping test is not exhaustive |
| CLI/Desk surfaces | Implemented with documented GUI residuals |
| Backward compatibility | Not proven by the required fixture/golden regression; default reader drift exists |
| Determinism | A shuffled-input test exists, but full required verification was not runnable |
| Source immutability | Integration coverage exists but was not executable |
| Documentation | Substantially implemented |
| DoD-15 verification gate | Unmet in the reviewed tree |
| DoD-16 closure/provenance | Unmet |

## Findings

[P1] Default-off scan can silently skip messages when new optional properties are malformed

Confidence: High

Requirement: DoD-1, DoD-10; zero silent change with all 0075 flags absent.

Location: `crates\pst-reader\src\messaging\message.rs:174-183`; `crates\pst-dedup-cli\src\scan.rs:560-577`

Problem: `delivery_time` and `display_bcc` are read unconditionally with `?`. Property-resolution errors propagate, and scan maps the resulting error to a skipped message. These reads occur even when every new policy flag is off.

Failure scenario: A previously recoverable message has corrupt/unresolvable `0x0E06` or `0x0E02`; pre-0075 scanning could retain it, while this implementation skips it and may change winners/default output.

Correction: Make these two new properties best-effort and return `None` on optional-property decode failure, while preserving existing failure behavior for required properties.

Verification: Add a fixture with only one optional property corrupted; compare default candidate sets, winners, and pre-existing output fields before and after 0075.

Deferrable: No

[P1] Required track closeout and provenance artifacts are incomplete

Confidence: High

Requirement: DoD-16.

Location: `conductor\0075-KeepSetWinnerPolicies\spec.md:400`; `conductor\conductor.md:176`; `conductor\sequencing.md:146`

Problem: The track has no `review.md`, no `D-0075-*` entries in `docs\deferred.md`, and remains marked `Ready`/upcoming rather than `Completed`. Ledgerful status and verification could not establish a committed transaction because its database/report storage is inaccessible.

Correction: Complete the track closeout, record residuals, update both registries, and commit/verify the Ledgerful transaction.

Verification: Re-run Ledgerful status/verification and confirm the closure artifacts and registry states.

Deferrable: No

[P2] Folder classification does not consistently apply the locked ladder precedence

Confidence: High

Requirement: DoD-2 and DoD-4; spec §3.4.

Location: `crates\dedup-engine\src\keepset.rs:921-984`

Problem: The classifier returns the first matching class from several loops rather than the best-ranked matching class. For example, `Archive/Junk Email` is classified as `junk_email` (rank 3) instead of `archive` (rank 2). Recoverable descendants are likewise selected by path order rather than best recoverable rank.

Failure scenario: A duplicate under `Archive/Junk Email` can tie or lose against a plain `Junk Email` copy, despite the specified archive preference.

Correction: Collect all valid matching classes and select the minimum built-in rank, retaining the Recoverable Items parent qualification.

Verification: Add overlapping-segment tests, including non-recoverable and recoverable paths, and assert both class and winner.

Deferrable: No

[P2] Valid positive pre-1970 FILETIMEs are ranked but serialized as missing

Confidence: High

Requirement: Spec §3.3; DoD-5.

Location: `crates\dedup-engine\src\keepset.rs:853-882`

Problem: `resolve_item_date` treats every positive FILETIME as valid, but `format_date_filetime_utc` returns an empty string when the converted Unix timestamp is negative. The decision row can therefore contain `date_source=submit` or `delivery` with an empty date.

Failure scenario: A legitimate pre-1970 message participates in `earliest_date` ranking but downstream consumers see an apparently missing date.

Correction: Format signed pre-epoch Unix seconds as ISO-8601 UTC; reserve empty output for `None` or FILETIME `<= 0`.

Verification: Add tests for a valid pre-1970 FILETIME and for zero/negative missing values.

Deferrable: No

[P2] Required honesty statistics are absent from human summaries

Confidence: High

Requirement: Spec §3.3 and DoD-6b require `winners_without_bcc_peer_had_bcc` and `groups_date_source_mixed` in JSON and human summaries.

Location: `crates\pst-dedup-cli\src\keep_set_cmd.rs:224-250`; `unique_eml_cmd.rs:380-414`; `unique_pst_cmd.rs:2096-2132`

Problem: The statistics are computed and serialized into keep-set JSON, but human summaries do not print either value. Only the recoverable-items hint is emitted.

Correction: Print all three run-level honesty statistics consistently on the human keep-set, unique-eml, and unique-pst surfaces.

Verification: Add CLI assertions for nonzero BCC-loss and mixed-date groups in human output and JSON output.

Deferrable: No

[P2] All-Custodians parity is wired but not proven across all required artifacts

Confidence: High

Requirement: DoD-6 and test item 8.

Location: `crates\dedup-engine\src\keepset.rs:3799-3848`; `crates\pst-dedup-cli\tests\unique_pst.rs:143-180`; `crates\pst-dedup-cli\src\unique_export_report.rs:872-883`

Problem: Engine tests cover aggregate construction and decision-row values, while CLI tests verify export headers and row counts. No test compares `duplicate_source_count`, `duplicate_sources`, and truncation semantics across decision CSV, `keep_set_v1` JSON, and `export_messages.csv`.

Correction: Add a synthetic multi-source integration test that asserts exact count, sorted basename list, cap-8 behavior, and truncation parity across all three outputs.

Verification: Run the new test through the CLI export path.

Deferrable: No

[P2] The required default compatibility golden regression is missing

Confidence: High

Requirement: Spec §3.9 and DoD-10.

Location: `crates\dedup-engine\src\keepset.rs:3965-3995`; `conductor\0075-KeepSetWinnerPolicies\spec.md:299`

Problem: The new “golden” test uses only two synthetic in-memory candidates. It does not run the existing `fixtures\aspose_outlook.pst`, compare against a checked-in winner list, or compare pre-0075 decision CSV columns byte-for-byte.

Correction: Add the required fixture-based baseline and exact pre-0075-column comparison.

Verification: Run the fixture test with all new flags absent and assert winner-set and legacy-column identity.

Deferrable: No

[P2] Required verification gate is not green or reproducible in the reviewed tree

Confidence: High

Requirement: DoD-15.

Location: `conductor\0075-KeepSetWinnerPolicies\spec.md:399`; `crates\dedup-engine\src\keepset.rs` formatter differences around lines 3164 and 3492

Problem: `cargo fmt --all --check` fails. Workspace clippy and tests fail before compilation because Cargo cannot open `target\debug\.cargo-build-lock` (`Access is denied`). `ledgerful verify` reports all three verification steps failed and cannot write its report.

Correction: Resolve the formatting differences and execution-environment lock, then rerun all three required commands successfully.

Verification: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; `ledgerful verify`.

Deferrable: No

## Completeness Sweep

The main implementation path is present: reader properties → scan candidates → `RankContext` resolution → decision CSV/keep-set JSON/export reporting.

No new placeholder or obvious no-op implementation was found. CSV columns are append-only, aggregate values are basename-capped, and the documented opt-in flags are generally default-inert. However, the findings above leave correctness, compatibility proof, required reporting, and closure incomplete.

## Wiring and Regression Review

- Ladder order is wired as fidelity → BCC → source → folder → policy.
- CLI surfaces share the ranking helper and expose the requested flags.
- Desk exposes earliest-date, folder-class, and BCC controls.
- JSON compatibility fields are additive and old keep-set JSON deserialization is covered.
- Graded fidelity has a current exhaustive match, but its tests omit tier-4/file-level cases.
- Source immutability coverage exists but could not execute.
- The default reader path violates the zero-silent-change requirement for malformed optional properties.
- Folder classification has valid-path edge cases that can change winners under the folder policy.

## Verification Evidence

- `git diff --check`: passed.
- `cargo fmt --all --check`: failed on formatting differences in `keepset.rs`.
- `cargo clippy --workspace --all-targets -- -D warnings`: blocked by Cargo lock access denial.
- `cargo test --workspace`: blocked by Cargo lock access denial.
- `ledgerful ledger status --compact`: unable to open Ledgerful database.
- `ledgerful scan --impact`: unable to write impact report.
- `ledgerful verify`: reported all three verification steps failed and could not write its report.
- Reviewer made no filesystem or Git changes.

## Deferred Candidates

None. No new P3 item is appropriate while P1/P2 correctness and completion requirements remain unresolved. The spec’s existing GUI residual is not a newly proposed deferral.

## Completion Decision

FAIL. The track is not complete due to a default-compatibility defect, incorrect folder-class edge behavior, missing human honesty statistics, incomplete required regression/parity proof, failed verification gates, and missing DoD-16 closure artifacts.