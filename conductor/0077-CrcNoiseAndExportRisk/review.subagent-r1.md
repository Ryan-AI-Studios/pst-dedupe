# Track Completion Audit — 0077-CrcNoiseAndExportRisk

## Verdict: FAIL

Core telemetry, log gating, `export_risk` vocabulary reuse, Desk banner, docs, and deferred closures are largely in place, but the track’s correctness payload (`CRC_SUSPECT` / D7) is incomplete and partially defeated by an unspec’d systematic-strip heuristic. Required DoD-19 proof tests and attachment-stream taint wiring are missing. Process DoDs 16–18 remain open.

## Scope Reviewed

| Artifact | Path |
|---|---|
| Spec / plan / notes | `conductor/0077-CrcNoiseAndExportRisk/{spec,plan,implementation-notes}.md` |
| Telemetry | `crates/pst-reader/src/integrity_telemetry.rs` |
| NDB warn sites | `crates/pst-reader/src/ndb/{page,block}.rs` |
| Message taint | `crates/pst-reader/src/messaging/message.rs` |
| Attachment stream | `crates/pst-reader/src/messaging/attachment.rs` |
| Scan attribution + strip | `crates/pst-dedup-cli/src/scan.rs` |
| Export risk | `crates/pst-dedup-cli/src/unique_export_report.rs` |
| Unique-pst wiring | `crates/pst-dedup-cli/src/unique_pst_cmd.rs` |
| CLI flags | `crates/pst-dedup-cli/src/{main,keep_set_cmd,unique_eml_cmd,unique_pst_cmd}.rs` |
| Keep-set / fidelity | `crates/dedup-engine/src/{integrity,keepset,grouping,hasher}.rs` |
| Writer cap | `crates/pst-writer/src/production.rs` |
| Desk banner | `crates/pst-dedup-gui/src/{unique_worker,views/unique_wizard}.rs` |
| Tests | `crates/pst-dedup-cli/tests/crc_integrity_0077.rs`, unit tests in telemetry / export_risk / keepset / unique_worker |
| Docs | `docs/{unique-pst-export,audit,deferred}.md` |
| Baseline | `conductor/0077-CrcNoiseAndExportRisk/baseline*.md/json` |

Branch (notes): `feat/0077-crc-noise-export-risk`. No `review.md`; conductor status left Ready (orchestrator).

---

## Requirement and DoD Matrix

| DoD | Status | Evidence |
|---|---|---|
| **1** integrity_telemetry module | **Met** | `integrity_telemetry.rs` TLS `Cell`s, global atomics, distinct-BID cap 1024 + `exact`, `snapshot`/`delta_since`/`reset`/`set_log_limit`/`flush_summary`, `TEST_LOCK` |
| **2** warn sites through gate | **Met** | `page.rs:108-112`, `block.rs:79-102`; no remaining CRC `tracing::warn!` outside gate |
| **3** bounded emission + exact total | **Met** | `bounded_emission_with_exact_total` (10k page CRC; emissions ≪ N; total exact) |
| **4** counters on FileScanStats/ScanSummary + serde default | **Met** | `scan.rs:113-191`, per-source delta `1156-1162`, rollup `1519-1564` |
| **5** rates + crc_skip_rate unchanged | **Met** | rates `1526-1529`; test `scan_reports_crc_fields_and_crc_skip_rate_unchanged` asserts `crc_skip_rate == 0.0` and `block_crc_read_rate ∈ [0,1]` |
| **19** CRC_SUSPECT via message-scope delta | **Partial / unmet** | Scope on `read_message_properties` / `read_message_extract` (`message.rs:207-267`, `292-334`); **not** on attachment streams (`attachment.rs:open_attachment_data`); taint uses **block CRC+BID only** (not page); **no E2E test** that a body-CRC-bad message is tainted while a clean sibling in the same file is not |
| **20** Tier-2 ineligible; Tier-1 MID unchanged; escape hatch | **Partial** | Logic: `keepset.rs:483-489`, `867-873`; scan `954-958`; flag on all five CLIs. **No test** proves MID-bearing suspect twin still groups, or that `--allow-crc-suspect-tier2` restores Tier-2 exactly |
| **21** fidelity tier arm | **Met** | `reason_fidelity_tier` includes `CrcSuspect` → 3 (`keepset.rs:1314`); DoD-21 unit asserts clean outranks suspect graded+binary (`4467-4481`) |
| **22** crc_suspect_messages per source + total + human | **Partial** | Fields + scan human line (`main.rs:1758-1767`). **Systematic strip zeros reported count** even when messages were tainted during the pass (`scan.rs:1132`) |
| **6** export_risk = PreflightRecommendation | **Met** | `ExportRisk.level: PreflightRecommendation` (`unique_export_report.rs:206-212`); no `low\|elevated\|high` enum found in crates |
| **7** monotone composition | **Met** | `scan_recommendation.max(post)` (`324`); unit `export_risk_monotone_composition` |
| **8** advisory vs catastrophic | **Met** | Units at attach 0.06 → re_export; read_rate 0.20 → not_export_ready; reasons name threshold/value |
| **23** Desk banner | **Met** | `unique_done_banner` + `show_done` green only for SuccessOk; stats row; unit `unique_done_banner_mapping` |
| **9** CLI flags both parsers | **Met** | Flags on scan/dups/keep-set/unique-eml (main) + unique-pst clap; applied via `set_log_limit`. No formal `--help` snapshot file; clap help text present |
| **10** synthetic corrupt fixture | **Partial** | Generate-at-test-time in `crc_integrity_0077.rs`; asserts mismatch **totals** and reads exist, **not** deterministic per-class counts (page vs block vs BID) as DoD text requires |
| **11** attach event cap | **Partial** | Cap 1000 + total/truncated on `WritePstReport` (`production.rs:661-716`). `ExportSection` fields always `None` in unique-pst (`unique_pst_cmd.rs:2111-2112`) — not surfaced on `unique_export_report_v1` |
| **12** clean corpus no behavior change | **Partial** | Baseline captured; aspose test asserts 17/17. Clean path may rely on page-out-of-taint and/or systematic strip rather than “no CRC hit ⇒ no taint”. Residual: streaming under-merge on poly stores before strip (notes) |
| **13** new lines numbers-only | **Met (review)** | New scan CRC line is numeric; telemetry flush uses counters/fields. No hostile-folder regression test |
| **14** unique-pst-export decision tree | **Met** | `docs/unique-pst-export.md` §CRC integrity: ScanPST mutate/copy, discard-on-repair, count-diff, classic Outlook, Purview unindexed≠corrupt, physical vs logical |
| **15** audit SEC-06 | **Met** | `docs/audit.md:341` “warning-only **and counted per source**”; not closed |
| **16** perf timing | **Unmet** | No before/after multi-GB or fixture timing in `review.md` (file absent); notes admit residual |
| **17** full gate | **Partial** | Notes: fmt/clippy + targeted package tests OK; **full** `cargo test --workspace` / `ledgerful verify` not fully re-run after last tweak |
| **18** review.md + deferred closed + conductor Completed | **Unmet** | No `review.md`; deferred D-0074/D-0073 **are** closed; D-0077-* residual rows present; conductor not Completed (by design left to orchestrator, still DoD-unmet) |

### Locked rules check (spec §2.3)

| Rule | Status |
|---|---|
| Count first, log second | Met — totals always incremented |
| Counters in data path | Met |
| One risk vocabulary | Met — `PreflightRecommendation` only |
| CRC warning-only, non-fatal | Met — validators still `Ok` after note_* |
| Bounded memory | Met for telemetry BIDs + attach Vec |
| Sources read-only | Met |
| New lines counters only | Met (scan); unique-pst human omits CRC/export_risk line (§3.10 partial) |
| Additive JSON / serde default | Met |
| No exit-code change | Met — no 0077 exit mapping; handoff 0078 |
| Recovered corruption still corruption | **Violated at ≥50% block CRC rate** by systematic strip |

---

## Findings

### [P1] Systematic 50% block-CRC strip undoes CRC_SUSPECT on heavily corrupt sources
**Confidence:** High

**Where:** `crates/pst-dedup-cli/src/scan.rs:1075-1132`

```text
SYSTEMATIC_BLOCK_CRC_RATE = 0.50
if block_crc_mismatches / block_reads >= 0.50 → strip CrcSuspect from candidates/rows,
  zero crc_suspect_reported
```

**Not in original spec.** Notes justify it as “aspose non-standard poly.” That poly class is documented on **page** CRC (`page.rs:104-106`), and message taint **already ignores page CRC** (`integrity_telemetry.rs:261-268`). A second, global block-rate strip is therefore poorly motivated for poly and **exactly removes D7 protection** when real block corruption is widespread (≥50% of block reads fail).

**Impact vs DoD-19/20 / rule 10:**
- Sparse real corruption still taints (threshold high) — residual claim is true for low rates.
- Massively corrupt media (the catastrophic class `export_risk` already keys at **0.15** read rate) can land at ≥0.50 block-only rate, strip taint, re-enable Tier-2 identity over suspect bytes after keep-set rebuild from cleaned candidates.
- Reported `crc_suspect_messages` becomes **0** while raw CRC counters remain — under-reports the kept-despite-CRC class.
- Integrity CSV may already have written `CRC_SUSPECT` degraded rows before strip → ledger vs final candidates diverge.
- Streaming index inserts with `tier2_eligible=false` before strip; keep-set rebuild from stripped candidates can re-merge — notes admit “streaming scan may still under-merge on poly stores.”

**export_risk** still sees raw rates (catastrophic at 0.15), so operators get a post-export risk signal, but **identity/taint is the D7 payload** this track exists for. Strip at 0.50 can re-open poison Tier-2 merges on the worst stores.

**Risk if kept:** Document as residual with narrower trigger (e.g. known poly allowlist / page-only class already handled), or gate strip so it never clears taint when `block_crc_read_rate` is in the real-corruption regime without a poly signal.

---

### [P1] DoD-19 incomplete: attachment streams not in message scope
**Confidence:** High

Spec §3.3a / DoD-19 require snapshot delta on `read_message_properties`, `read_message_extract`, **and attachment stream reads**.

- Properties/extract: wired (`message.rs:207`, `292`).
- `open_attachment_data` / leaf block stream: **no** `message_scope_enter` (`attachment.rs:212+`).
- Scan reads attachment **metadata** after message scope has already exited (`scan.rs:762-764` after `read_message_properties` returned) — CRC during attach-meta walks never taints.

Consequence: clean body + corrupt attach payload can ship without `CRC_SUSPECT` even though bytes used for export are suspect. D7 remains open on the attach path.

---

### [P1] DoD-19 / verification #7 unproven: no scoped taint integration test
**Confidence:** High

Required: in one corrupt file, message over bad body block → `CRC_SUSPECT`; message over only clean blocks → not tainted.

Present:
- Unit: `message_scope_detects_delta` (synthetic `note_block_crc` / page exclusion) — `integrity_telemetry.rs:577-589`.
- Integration: `synthetic_corrupt_pst_increments_specific_crc_counters` asserts only that **some** mismatch total > 0 — **not** per-class determinism, **not** `crc_suspect` on any message.

Absent: end-to-end scan/read asserting `props.crc_suspect` / degraded reasons for two NIDs in one fixture.

Without that proof, DoD-19 cannot be marked met even if logic is roughly right.

---

### [P2] Page CRC excluded from message taint — deliberate; D7 partially open
**Confidence:** High

Spec §3.3a: B-tree page CRC during a message read **should** count toward that message (over-inclusive, split-safe).

Implementation: `tls_block_mismatch_total` = block CRC + BID only (`integrity_telemetry.rs:261-268`), with explicit comment that page CRC must not taint (poly / DoD-12).

**Justification:** real fixtures use non-standard **page** poly (`page.rs:104-106`); including page would taint every message on “clean” aspose and break DoD-12. That is a reasonable product compromise **if** documented as residual and if block taint is solid.

**Residual D7 gap:** true page/B-tree corruption (not poly) can still feed wrong structure while messages look clean. Partially open by design — not a silent bug, but not full DoD-19 text.

---

### [P2] DoD-20 unproven by tests (logic present)
**Confidence:** High

Tier-2 gate and MID Tier-1 path are implemented in keep-set/scan/GUI. Spec verification #8 / DoD-20 require tests:
- suspect + readable MID still groups with clean twin (Tier 1),
- `--allow-crc-suspect-tier2` restores pre-0077 Tier-2 eligibility exactly,
- split-only refinement on corrupt runs.

No such tests found under `dedup-engine` or `pst-dedup-cli` tests.

---

### [P2] DoD-11: cap implemented; `_total` / `_truncated` not surfaced on export summary
**Confidence:** High

Writer caps and totals (`production.rs:661-716`, report fields `498-501`). Unique-pst always sets:

```text
attachment_fidelity_events_truncated: None
attachment_fidelity_events_total: None
```

(`unique_pst_cmd.rs:2111-2112`, cancelled path `801-802`). Spec §3.10 puts these on `ExportSection`. Cap closes OOM class; operator report surface incomplete.

---

### [P2] DoD-10 weak: synthetic fixture does not assert per-class counters
**Confidence:** High

DoD-10: deterministic **page-CRC, block-CRC, and BID-mismatch** counts. Test only asserts sum > 0 and reads > 0. Plan risk “assert specific counters, not just some warning” is unmet.

---

### [P2] DoD-16 / DoD-17 / DoD-18 process gaps
**Confidence:** High

- **16:** no recorded page-heavy before/after timing in review.
- **17:** implementer notes full workspace test not fully re-run after last telemetry tweak; `ledgerful verify` not evidenced here.
- **18:** no `review.md`; conductor not Completed; deferred closures for D-0074/D-0073/D-0077-* **are** done in `docs/deferred.md`.

---

### [P3] unique-pst human summary omits CRC / export_risk line
**Confidence:** High

Spec §3.10: human line on scan **and** unique-pst with counts, distinct BIDs, exactness, `export_risk`. Scan has CRC line (`main.rs:1758-1767`). Unique-pst human block (`unique_pst_cmd.rs:2248-2299`) has no `export_risk` or CRC counters (JSON carries `export_risk`).

---

### [P3] Missing hostile-folder / untrusted-string test for new lines
**Confidence:** Medium

Spec verification #6. New lines look numeric; no automated assertion that `\x1b[31m…` does not appear on new CRC summary lines.

---

### [P3] Missing two-source attribution integration test
**Confidence:** Medium

Plan Phase 4: corruption only in second source → file[0] zero, file[1] non-zero. Snapshot site comment for D-0077-parallel-attrib present (`scan.rs:523-525`); no test found.

---

### [P3] DoD-6 “workspace grep” test is a stub
**Confidence:** High

`no_competing_risk_enum_vocabulary` only asserts `PreflightRecommendation::as_str` strings; does not grep the workspace. Manual grep found no competing enum — functionally OK, weak proof.

---

## Completeness Sweep

| Area | Result |
|---|---|
| Placeholders / stubs | No TODO stubs in core 0077 paths. ExportSection attach-event fields effectively stubbed as always-`None`. |
| End-to-end wiring | Telemetry → scan fields → preflight rates → unique-pst `export_risk` → Desk banner: wired. Attach-event totals → ExportSection: **not** wired. Attach stream → CRC_SUSPECT: **not** wired. |
| Placeholders in docs | Decision tree and SEC-06 look complete; deferred residuals recorded. |
| Exit codes | Unchanged (correct). |
| `crc_skip_rate` | Meaning preserved (message skips only). |
| CRC warning-only | Preserved. |

---

## Wiring and Regression Review

### Core path (met)
1. NDB validate → `note_*` → TLS + gate → `snapshot`/`flush_summary`
2. Scan per-source delta → `FileScanStats` / `ScanSummary` rates
3. `props.crc_suspect` → `IntegrityReason::CrcSuspect` → Tier-2 block (unless flag) → fidelity tier 3
4. `compute_export_risk` max(scan, post) → `UniqueExportSummary.export_risk` → GUI `UniqueOutcomeView` → banner

### Regression hotspots
| Concern | Assessment |
|---|---|
| Systematic strip @ 50% | **High risk** to DoD-19/20 on high block-CRC-rate stores; sparse OK; poly already mitigated by page exclusion |
| Page CRC not tainting | Justified for poly; residual D7 for real page corruption |
| export_risk uses PreflightRecommendation | Correct; no low\|elevated\|high |
| crc_skip_rate | Unchanged; pinned by test |
| Exit codes | No change |
| CRC warning-only | Unchanged |
| Attach events cap | Cap works in-process; report surface incomplete |

---

## Verification Evidence

**From implementation-notes (session claims; not re-run in this review):**

```text
cargo fmt --all --check          OK
cargo clippy --workspace --all-targets -- -D warnings   OK
cargo test -p pst-reader         OK
cargo test -p dedup-engine       OK (154)
cargo test -p pst-writer         OK (30)
cargo test -p pst-dedup-cli      OK (incl. crc_integrity_0077, keep_set aspose, unique_pst)
cargo check -p pst-dedup-gui     OK
cargo test -p pst-dedup-gui      OK (22; unique_done_banner_mapping)
```

**Gaps vs DoD-17:** full `cargo test --workspace` and `ledgerful verify` not claimed after last telemetry tweak; this reviewer did not re-execute the full gate.

**Test coverage vs DoD proof requirements:**

| Required proof | Present? |
|---|---|
| Bounded emission + exact total | Yes (unit) |
| crc_skip_rate unchanged + rates | Yes (integration) |
| Taint precision same-file clean vs bad | **No** |
| MID Tier-1 + allow-crc-suspect-tier2 | **No** |
| Fidelity clean > suspect | Yes (unit) |
| export_risk monotone / advisory / catastrophic | Yes (unit) |
| Desk banner mapping | Yes (unit) |
| Per-class synthetic CRC counts | **No** (weak sum>0 only) |
| Two-source attribution | **No** |
| Clean corpus content_hash byte-identical | Partial (counts only on aspose) |

---

## Deferred Candidates

Already recorded (OK):

- `D-0077-tracing-layer`, `D-0077-parallel-attrib`, `D-0077-desk-subscriber`, `D-0077-gui`, `D-0077-repair-diff`
- Closed: `D-0074-crc-fixture`, `D-0073-vec-events`

**Should be residual (not currently explicit enough):**

| ID (proposed) | Severity | Item |
|---|---|---|
| D-0077-systematic-strip | P1 | 0.50 block-CRC strip policy: document/narrow or remove; never silent taint clear on real high-rate corruption |
| D-0077-attach-scope | P1 | Message-scope taint around attachment stream / attach-meta reads |
| D-0077-taint-tests | P1/P2 | Same-file clean vs suspect + MID Tier-1 + allow-flag tests |
| D-0077-export-attach-surface | P2 | Wire `attachment_fidelity_events_{total,truncated}` into ExportSection |
| D-0077-page-taint | P2/P3 | Document page-CRC exclusion residual vs full §3.3a |
| D-0077-perf | P2 | DoD-16 timing record |

---

## Completion Decision

**Verdict: FAIL**

### Why not PASS / PASS WITH DEFERRED P3
1. **P1 systematic strip** can zero `CRC_SUSPECT` and re-open Tier-2 on stores with ≥50% block CRC failure — contradicts rule 10 and the track’s D7 rationale; not an original-spec residual.
2. **P1 DoD-19 wiring gap** on attachment streams.
3. **P1 missing proof tests** for scoped taint (the correctness bar the plan called out as “if the track is cut short, this is what must survive”).
4. Multiple **P2** contract/surface gaps (attach event report fields, weak DoD-10, process 16–18).

### What is solid enough to keep
- Data-path telemetry + bounded emission (D1/D2 flood fix)
- Per-source CRC counters and rates; `crc_skip_rate` meaning held
- `export_risk` on existing vocabulary; monotone; advisory vs catastrophic tests
- Desk banner no longer unqualified green success
- Docs decision tree + SEC-06 observability update + deferred closes for rolled-in P3s
- Fidelity arm for `CrcSuspect` (DoD-21)
- CRC remains warning-only; no exit-code change

### Minimum to re-grade PASS or PASS WITH DEFERRED P3
1. Remove or tightly redesign systematic strip so real high-rate block corruption **never** loses taint; if poly detection remains, isolate it from D7 clearing (and never zero `crc_suspect_messages` without an explicit poly code).
2. Scope attachment stream (and ideally attach-meta during scan) for CRC_SUSPECT.
3. Add integration tests: same-file clean vs body-bad taint; MID Tier-1 merge with suspect twin; `--allow-crc-suspect-tier2`; optional two-source attribution.
4. Surface attach event total/truncated on ExportSection from writer reports.
5. Close DoD-16 record + full gate + `review.md` / conductor flip (orchestrator may own 18).

### Systematic-CRC heuristic — explicit risk summary
| Question | Answer |
|---|---|
| In original spec? | **No** |
| Justified? | Partially for **page** poly only; block-rate strip is redundant with page exclusion and over-broad for real corruption |
| Violates DoD-19/20? | **Yes at ≥50% block CRC rate** (taint cleared, Tier-2 can re-bind); sparse real corruption still OK |
| Violates DoD-12 clean corpus? | Intended to *help* clean poly stores; with page-out-of-taint may be unnecessary for aspose |
| export_risk? | Still sees raw counters/rates — advisory/catastrophic paths intact |
| Recommendation | Treat as **blocking** correctness issue for track complete, not a P3 residual |
