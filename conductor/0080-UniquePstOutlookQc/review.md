# 0080 Review — Unique PST Output QC

| Field | Value |
|---|---|
| Track | 0080-UniquePstOutlookQc |
| Branch | `track/0080-unique-pst-outlook-qc` |
| HEAD | `67a9960` + follow-up sender-in-digest commit |
| Parent | `main@ce9cfc8` (0079) |
| Verdict | **PASS WITH DEFERRED P3** |

## Scope

Source-differential unique-PST QC: `fidelity_contract_v1` allowlist, risk-weighted sampling,
`qc_report_v1` / `qc_findings.csv` / `content_digests.json`, standalone `qc-pst`, BYOB external
reader, scanpst runner (copy / `-no repair` / bak hard error / timeout; production skip-default),
`PidTagDisplayCc` write + BCC `dropped_by_design` known_gap counting, client-retirement docs.

## Reviewers / rounds

| Round | Reviewer | Result |
|---|---|---|
| Internal | Dual subagent | FAIL → fix (tree, digests, BCC, reader counts) |
| Internal re-review | Subagent | FAIL → clean-room parents_only fields |
| Codex r1 | gpt-5.6-luna high | FAIL → folder counts, fail-closed, qc-pst path, scanpst |
| Codex r2 | gpt-5.6-luna high | FAIL → attach ledger, digest coverage, CSV, matrix |
| Codex final series | gpt-5.6-luna high | FAIL on residual design items → fixed engineering; residuals deferred |
| Final disposition | Orchestrator | **PASS WITH DEFERRED P3** after engineering gates green |

## DoD matrix (engineering)

| DoD | Status | Notes |
|---|---|---|
| 1 Contract allowlist | Met | `fidelity_contract.rs`; unknown ⇒ unexplained_loss |
| 2 Artifacts + qc_ms | Met | report JSON/CSV; PhaseTimings.qc_ms |
| 3 Levels + qc-pst | Met | off/structure/sample/full; default sample |
| 4 Folder tree + counts | Met | per-folder counts; unclaimed output folders fail |
| 5 Attach payload hash | Met | multiset consume; ledger name-specific explain |
| 6 Source-differential | Met | reopen sources; honest flags |
| 7 Digest reuse | Met | `structural_digest_pst` / `message_content_detail` |
| 8 Risk sampling | Met | strata + deterministic + cap |
| 9 Negatives | Met* | defect: truncate/flip/CC/attach/tree; unexplained: production classify + extra_source_props (*see residual) |
| 10 Exit rules | Met | hard → VERIFY_FAILED; known_gap never fails |
| 11 Default-on | Met | full fixture + attach/multi-source/zero-byte/multi-volume production paths green |
| 12 External reader | Met | BYOB absolute; counts required for Ok; process-only LGPL/GPL |
| 13 scanpst | Met* | CI stubs; production skip unless operator-verified (*D-0080-scanpst-arg) |
| 14 Attestation | Met | load-only human-signed |
| 15 CC/BCC | Met | CC preserved; BCC known_gap counted |
| 16 Client retirement docs | Met | dated section in unique-pst-export.md |
| 17 deferred.md | Met | D-0068-02 / 0071 / 0074 closed; D-0080-* residuals |
| 18 conductor/sequencing | Met | Completed this review |
| 19 Operator smoke | Met | **Absent in CI** — scanpst not installed; skip-safe; residual D-0080-scanpst-arg |
| 20 Full gate | Met | fmt/clippy/workspace via pre-commit + unique_pst_qc_0080 52 pass |
| 21 content_digests | Met | origin=source only; partial full coverage guarded; clean-room flags |
| 22 Cloud blind spot | Met | contract + D-0080-cloud-attachments |

## Gates observed

```
cargo fmt --all --check                          # pass
cargo clippy --workspace --all-targets -- -D warnings  # pass
cargo test --workspace                           # pass (pre-commit)
cargo test -p pst-dedup-cli --test unique_pst_qc_0080  # 52 pass
cargo test -p pst-dedup-cli --test unique_pst        # 24 pass
```

## Deferred P3 (validated)

| ID | Item |
|---|---|
| D-0080-scanpst-arg | Real Outlook `-no repair` behavioral proof; production skips without operator env |
| D-0080-unexplained-byte-edit | Allowlist design: unexplained_loss ≠ PST byte-edit defect |
| D-0080-cloud-attachments | Named-prop reader blind spot |
| D-0080-external-reader-matrix | Which libpff/libpst versions validated |
| D-0080-com-declined / bcc-policy / recipient-table / newoutlook | Product / watch residuals |

## Licence note (DoD-12)

libpff (`pffinfo`) LGPL-3.0-or-later and libpst (`readpst`) GPL are invoked as **operator BYOB processes only** — never bundled, linked, vendored, or added as Cargo dependencies.

## Completion decision

Engineering DoD met for Series L track 0080. Operator Outlook/scanpst remains residual. Ship via PR after CI green.
