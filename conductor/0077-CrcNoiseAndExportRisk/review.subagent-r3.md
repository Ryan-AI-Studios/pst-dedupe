# Track Completion Audit r3 — 0077-CrcNoiseAndExportRisk

**Branch:** `feat/0077-crc-noise-export-risk`  
**Date:** 2026-07-28  
**Reviewer:** subagent r3 (read-only re-review after r2 fix round)  
**Prior:** `review.subagent-r2.md` (**FAIL** — four P2 residuals), fix round documented in `implementation-notes.md` §“Fix round r2”

---

## Verdict: **PASS WITH DEFERRED P3**

All four r2 **P2** residuals are closed or legitimately demoted:

| r2 P2 | Disposition |
|---|---|
| Block-only ≥0.50 identity strip | **Closed** → dual-rate poly gate; residual true-fingerprint / dual-failure edge → **P3** `D-0077-systematic-poly` (accepted product residual for aspose DoD-12) |
| integrity.csv not reconciled after strip | **Closed** — buffer per-file, flush post-strip; skip rows still stream |
| Attach payload `crc_suspect` not consumed | **Closed** — materialize + attach_probe consumers OR into message `CRC_SUSPECT` |
| DoD-16 timing unrecorded | **Closed as fixture-scale evidence** (~39 ms aspose release); multi-GB / ≤2% ceiling → **P3 residual** |

No remaining **P0/P1/P2** product defects. Residuals are P3 (documented product/process polish) only.

---

## Scope Reviewed

| Artifact | Path |
|---|---|
| Spec / notes / r1–r2 reviews | `conductor/0077-CrcNoiseAndExportRisk/{spec,implementation-notes,review.subagent-r1,r1b,r2}.md` |
| Dual-rate strip + integrity buffer | `crates/pst-dedup-cli/src/scan.rs` |
| Materialize / attach probe | `crates/pst-dedup-cli/src/{pst_materializer,attach_probe}.rs` |
| Telemetry / attach reader | `crates/pst-reader/src/{integrity_telemetry.rs,messaging/attachment.rs,ndb/{page,block}.rs}` |
| Keep-set / fidelity | `crates/dedup-engine/src/keepset.rs` |
| Export risk / unique-pst | `crates/pst-dedup-cli/src/{unique_export_report,unique_pst_cmd}.rs` |
| Writer cap | `crates/pst-writer/src/production.rs` |
| GUI | `crates/pst-dedup-gui/src/{worker,unique_worker,views/unique_wizard}.rs` |
| Tests | `crates/pst-dedup-cli/tests/crc_integrity_0077.rs`, scan unit `poly_class_requires_dual_high_rate` |
| Docs | `docs/{unique-pst-export,audit,deferred}.md` |

**Method:** static file:line verification of every r2-fix claim. Runtime gate not re-executed in this r3 session (no shell in review environment); implementation-notes r2 gate claims retained as secondary evidence. `target/release/pst-dedup.exe` present on disk.

---

## r2 fix-claim verification

### 1. Dual-rate poly strip — **MET**

**Claim:** `poly_class = (block≥0.5) AND (page≥0.5)` only; high block alone keeps `CRC_SUSPECT`.

**Evidence:**

```1074:1099:crates/pst-dedup-cli/src/scan.rs
        // Poly-class identity strip (D-0077-systematic-poly / DoD-12 aspose):
        // ...
        //   poly_class = (block_crc/block_reads ≥ 0.50) AND (page_crc/page_reads ≥ 0.50)
        // High block rate alone (real data-block corruption) **keeps** CRC_SUSPECT
        let poly_class = is_poly_class_crc(
            tel.page_crc_mismatches,
            tel.page_reads,
            tel.block_crc_mismatches,
            tel.block_reads,
        );
        // ...
        if poly_class && file_crc_suspect > 0 {
```

```1615:1625:crates/pst-dedup-cli/src/scan.rs
pub(crate) fn is_poly_class_crc(
    page_crc: u64,
    page_reads: u64,
    block_crc: u64,
    block_reads: u64,
) -> bool {
    const POLY_RATE: f64 = 0.50;
    let page_rate = page_crc as f64 / (page_reads.max(1) as f64);
    let block_rate = block_crc as f64 / (block_reads.max(1) as f64);
    page_rate >= POLY_RATE && block_rate >= POLY_RATE
}
```

Unit proof (high block low page **false**; both high **true**):

```1757:1770:crates/pst-dedup-cli/src/scan.rs
    fn poly_class_requires_dual_high_rate() {
        assert!(is_poly_class_crc(50, 100, 50, 100));
        assert!(is_poly_class_crc(1, 1, 1, 1));
        // High block, low page → real data-block corruption; keep CRC_SUSPECT.
        assert!(!is_poly_class_crc(1, 100, 50, 100));
        assert!(!is_poly_class_crc(0, 100, 100, 100));
        assert!(!is_poly_class_crc(50, 100, 1, 100));
        assert!(!is_poly_class_crc(1, 100, 1, 100));
        assert!(!is_poly_class_crc(0, 0, 0, 0));
    }
```

**No** surviving block-only `SYSTEMATIC_BLOCK_CRC_RATE` gate (grep: only dual-rate `0.50` comments/helper).

Pre-strip telemetry honesty retained (`crc_suspect_reported = file_crc_suspect` at `scan.rs:1098`; `FileScanStats.crc_suspect_messages` at `1191`). Raw page/block counters never zeroed.

**Product residual (P3, accepted):** dual-rate is a heuristic, not a true poly allowlist/fingerprint. A store with *real* widespread page **and** block corruption could still clear identity taint while raw rates still drive `export_risk` (catastrophic `block_crc_read_rate ≥ 0.15` independent of strip — `scan.rs:1085-1086`). Documented at `docs/deferred.md:814`.

---

### 2. integrity.csv buffered then flushed after strip — **MET**

**Claim:** degraded integrity rows buffered per-file; flushed after strip; strip filters `CRC_SUSPECT`; skip rows still stream.

**Evidence:**

Buffer declare + intent:

```634:636:crates/pst-dedup-cli/src/scan.rs
        // Buffer degraded integrity rows until end-of-source so poly-class identity
        // strip can reconcile integrity.csv with candidates (0077 r2 P2-2).
        let mut file_integrity_degraded: Vec<SkipRecord> = Vec::new();
```

Push during message loop (no immediate write for degraded):

```844:856:crates/pst-dedup-cli/src/scan.rs
                            // Buffer degraded ledger rows (one per reason). Flushed after
                            // poly-class strip so integrity.csv matches keep-set identity.
                            for r in &integrity.degraded_reasons {
                                file_integrity_degraded.push(SkipRecord { ... });
                            }
```

Strip reconciles buffer + degraded tallies:

```1134:1150:crates/pst-dedup-cli/src/scan.rs
            // Reconcile integrity.csv buffer with identity strip (P2-2).
            file_integrity_degraded.retain(|r| r.reason != IntegrityReason::CrcSuspect);
            // Reconcile file degraded tallies: remove CRC_SUSPECT contribution from
            // degraded_* maps (identity strip), not from crc_suspect_messages telemetry.
            ...
            // Index inserts already used eligible=false for CRC_SUSPECT; keep-set rebuilds
            // from candidates (identity-stripped). Streaming unique counts may under-merge
            // on poly stores until rebuild — keep-set rebuild is authoritative.
```

Flush post-strip:

```1153:1163:crates/pst-dedup-cli/src/scan.rs
        // Flush buffered degraded integrity rows (post-strip when poly_class).
        if let Some(wtr) = integrity_wtr.as_mut() {
            for row in &file_integrity_degraded {
                wtr.write_degraded(row) ...
            }
        }
```

Skip path still streams immediately via `record_skip` → `wtr.write_skip` (`scan.rs:1647-1651`) — crash resilience claim holds.

**P3 residual (documented):** streaming index eligibility not rewritten in-place; keep-set rebuild is authoritative; poly under-merge until rebuild named in comments + `D-0077-systematic-poly`.

---

### 3. materialize / attach_probe consume `crc_suspect` — **MET**

**Claim:** extract/props/attach stream → message `CRC_SUSPECT`; probe surfaces warning-only `CrcSuspect`; consumers `push_degraded`.

**Reader instrumentation (prior, still present):**

```104:110:crates/pst-reader/src/messaging/attachment.rs
                    // 0077: attribute leaf-block CRC/BID to this attach stream.
                    let before = crate::integrity_telemetry::tls_block_mismatch_total();
                    *chunk = block::read_leaf_block_data(...);
                    if crate::integrity_telemetry::tls_block_mismatch_total() > before {
                        self.crc_suspect = true;
                    }
```

```254:261:crates/pst-reader/src/messaging/attachment.rs
        let scope = crate::integrity_telemetry::message_scope_enter();
        ...
                if open_suspect {
                    reader.crc_suspect = true;
                }
```

**Materializer consumption:**

| Path | Lines |
|---|---|
| `read_message_extract` → soft `CrcSuspect` | `pst_materializer.rs:191-195` |
| props fallback → soft `CrcSuspect` | `pst_materializer.rs:234-238` |
| probe cache hit `ok && reason == CrcSuspect` | `pst_materializer.rs:327-330` |
| deep `probe_attach_stream` → soft `CrcSuspect` | `pst_materializer.rs:374-379` |
| `open_attachment_data` + `read_to_end` success | `pst_materializer.rs:395-401` |
| partial stream read still taints | `pst_materializer.rs:412-417` |

**attach_probe production of reason:**

```729:744:crates/pst-dedup-cli/src/attach_probe.rs
    if level == ProbeLevel::Open {
        let crc = reader.crc_suspect();
        ...
            reason: if crc {
                Some(IntegrityReason::CrcSuspect)
            } else { None },
```

```814:826:crates/pst-dedup-cli/src/attach_probe.rs
    // 0077: consume AttachmentDataReader::crc_suspect after stream read.
    let crc = reader.crc_suspect();
    ProbeOutcome {
        ok: true,
        reason: if crc { Some(IntegrityReason::CrcSuspect) } else { None },
```

**Consumers (not attach-fail rate):**

```1113:1119:crates/pst-dedup-cli/src/attach_probe.rs
            if let Some(r) = outcome.reason {
                if r.is_attach_probe_fail() {
                    let _ = apply_probe_fail(item, r, mode);
                } else if r == IntegrityReason::CrcSuspect {
                    // Warning-only attach-stream CRC → message CRC_SUSPECT (DoD-19 / D7).
                    push_degraded(item, IntegrityReason::CrcSuspect);
                }
```

Peer probe: `attach_probe.rs:1284-1290` same pattern.

Scan attach-meta still ORs via `with_crc_scope` (`scan.rs:766-770` region; `integrity_telemetry.rs:298-305`).

**DoD-19 attach stream path is no longer instrumented-but-orphaned.**

---

### 4. DoD-16 timing recorded — **MET (fixture-scale; multi-GB residual P3)**

**Claim:** ~39 ms aspose release scan recorded.

**Evidence:** `implementation-notes.md:112-119`:

```
target\release\pst-dedup.exe scan fixtures/aspose_outlook.pst --json
wall ≈ 0.039 s (TotalMilliseconds 38.5)
```

Phase-0 baseline duration was ~0.017 s via `cargo run` (`baseline.md:27`) — **not** same harness, so ≤2% / +5% ceiling is **not** proven apples-to-apples. Multi-GB page-heavy ceiling unproven.

**Disposition:** recording requirement for r2 P2 is satisfied; ceiling/multi-GB remains **P3 residual** (not a product defect). Spec DoD-16 fully closed only after orchestrator accepts fixture-scale or runs comparable before/after.

---

### 5. Sparse test clean twin assert — **MET**

```220:241:crates/pst-dedup-cli/tests/crc_integrity_0077.rs
    // Clean twin: sparse single-block flip must not taint every message in the file.
    if outcome.candidates.len() >= 2 {
        let clean_count = outcome.candidates.iter().filter(|c| {
            !c.integrity.degraded_reasons
                .contains(&IntegrityReason::CrcSuspect)
        }).count();
        assert!(clean_count >= 1, "expected at least one clean sibling without CRC_SUSPECT; ...");
        assert!(clean_count < outcome.candidates.len(), "expected mixed taint ...");
    }
```

Also: rate &lt; 0.50 so dual-rate strip cannot fire (`186-193`); `crc_suspect_messages > 0` + at least one suspect candidate (`200-218`). Closes r2 P3 half-open DoD-19 precision.

---

### 6. `D-0077-systematic-poly` demoted to P3 — **MET**

```814:814:docs/deferred.md
| D-0077-systematic-poly | P3 | Poly-class dual-rate identity strip for DoD-12 aspose | **Policy (r2):** strip only when **both** `block_crc/block_reads ≥ 0.50` **and** `page_crc/page_reads ≥ 0.50` (`is_poly_class_crc`). High block alone keeps `CRC_SUSPECT` ... Residual: true poly allowlist / fingerprint vs dual-rate heuristic | residual / product |
```

Severity **P3** (was P2 in r2). Dual-rate policy + integrity buffer + keep-set authority documented. Accepted product residual for aspose DoD-12 per review charter.

---

## Prior P1 / P2 disposition summary

| Finding | r1 | r2 | r3 |
|---|---|---|---|
| Systematic identity strip | P1 | P2 (block-only) | **P3** dual-rate + deferred row |
| Attach-meta / stream scope | P1 | meta met; stream consumer P2 | **Met** (consumers wired) |
| Sparse taint E2E + clean twin | P1 / P3 | met + P3 twin | **Met** (twin assert) |
| integrity.csv / index reconcile | — / P2 | P2 | **Met** (csv); index under-merge **P3** |
| DoD-20 units | P2 | Met | Met |
| ExportSection attach totals | P2 | Met success | Met success; cancel None **P3** |
| DoD-16 timing | unmet | P2 | Fixture recorded; multi-GB **P3** |
| GUI scan CRC_SUSPECT | P2 | Met | Met (`worker.rs:318-326`) |
| export_risk scan_preflight reason | P3 | Met | Met (`unique_export_report.rs:259-262`) |

---

## Full DoD matrix (r3)

| DoD | Status | Evidence |
|---|---|---|
| **1** telemetry module | **Met** | `integrity_telemetry.rs`: TLS/global, BID cap 1024 + exact, snapshot/delta/reset/set_log_limit/flush_summary |
| **2** warn sites gated | **Met** | `page.rs:111`, `block.rs:81`, `block.rs:98` → `note_*`; CRC warns only in telemetry gate |
| **3** bounded emission | **Met** | unit `bounded_emission_with_exact_total` |
| **4** scan counters + serde default | **Met** | `FileScanStats` / `ScanSummary` CRC fields; pre-0077 JSON deserializes via defaults |
| **5** rates + crc_skip_rate pin | **Met** | integration `scan_reports_crc_fields_and_crc_skip_rate_unchanged` |
| **19** CRC_SUSPECT scopes | **Met** | props/extract scope; attach-meta `with_crc_scope`; attach stream flag + materialize/probe consumers; clean twin assert |
| **20** Tier-2 / MID / allow | **Met** | keepset units ~`4486-4577`; Tier-1 MID untouched |
| **21** fidelity arm | **Met** | `reason_fidelity_tier` maps `CrcSuspect` → tier 3 (`keepset.rs:1314`); clean &gt; suspect units |
| **22** crc_suspect_messages | **Met** | per-file + total; **pre-strip** honesty; human scan line `main.rs:1765` |
| **6** export_risk vocabulary | **Met** | `PreflightRecommendation` only; unit comment DoD-6 no `low\|elevated\|high` |
| **7** monotone composition | **Met** | unit + F7 `scan_preflight=re_export_recommended` |
| **8** advisory vs catastrophic | **Met** | units 0.06 attach / 0.20 read rate |
| **23** Desk banner | **Met** | green only when risk Ok; `unique_done_banner_mapping` |
| **9** CLI flags both parsers | **Met** | `--crc-log-limit` / `--crc-log-interval-secs` / `--allow-crc-suspect-tier2` on scan/dups/keep-set/unique-eml/unique-pst |
| **10** synthetic corrupt fixture | **Mostly met** | page+block classes asserted separately; BID class soft → **P3** |
| **11** attach event cap + surface | **Met** | cap 1000; ExportSection totals success path `unique_pst_cmd.rs:1830-1832,2118-2119`; cancel path None → **P3** |
| **12** clean corpus / split-only | **Met with accepted residual** | aspose 17/17 golden path + dual-rate poly strip for poly-class fixtures; sparse real corruption keeps taint; high-block-alone keeps taint; true poly fingerprint residual **P3** |
| **13** numbers-only new lines | **Met** | scan CRC line; unique-pst `export_risk: <level>` codes-only (`unique_pst_cmd.rs:2279`) |
| **14** decision tree docs | **Met** | `docs/unique-pst-export.md` ScanPST / Purview / physical vs logical |
| **15** SEC-06 | **Met** | `docs/audit.md:341` warning-only **and counted per source**; not claimed closed |
| **16** perf timing | **Partial → P3 residual** | fixture-scale ~39 ms recorded; multi-GB / ≤2% ceiling not proven |
| **17** full gate | **Unverified in r3 runtime** | notes claim targeted packages green; full workspace + ledgerful → **orchestrator / process P3** |
| **18** review.md + conductor Completed | **Unmet process** | orchestrator owns `review.md` + conductor flip |

### Locked rules (§2.3)

| Rule | Status |
|---|---|
| Count first, log second | **Met** |
| Counters in data path | **Met** |
| One risk vocabulary | **Met** |
| CRC warning-only / non-fatal | **Met** |
| Bounded memory | **Met** (BID set + attach Vec) |
| Sources read-only | **Met** |
| New lines counters/codes only | **Met** |
| Additive JSON / serde default | **Met** |
| No exit-code change | **Met** |
| Recovered corruption still corruption | **Met for sparse + high-block-alone**; dual-rate poly-class identity strip is **documented accepted residual** (rule softens only under dual-rate heuristic for aspose DoD-12; telemetry + export_risk rates still honest) |

---

## Findings (r3)

### No P0 / P1 / P2 findings

### [P3] Dual-rate poly strip is heuristic (D-0077-systematic-poly)
**Where:** `scan.rs:is_poly_class_crc`, strip at `1099+`  
**Deferred:** `docs/deferred.md:814` severity **P3**  
True poly fingerprint/allowlist not shipped. Dual high page+block real corruption could clear identity while rates still elevate `export_risk`. Streaming unique may under-merge until keep-set rebuild. **Accepted product residual** for aspose DoD-12.

### [P3] DoD-16 multi-GB / comparable before-after ceiling
Fixture-scale only (~39 ms). Pre-0077 baseline not same harness. Multi-GB residual for operators.

### [P3] DoD-10 BID-mismatch class not hard-asserted
Page + block classes asserted; BID soft.

### [P3] unique-pst human summary omits full CRC counter line
Only `export_risk` level; scan has full CRC numbers line.

### [P3] Cancel-path ExportSection attach event fields remain `None`
`unique_pst_cmd.rs:801-802`.

### [P3] Page-CRC excluded from message taint
Deliberate poly policy (`integrity_telemetry.rs:261-274`); real page-structure risk without item taint.

### [P3] Process: DoD-17 / DoD-18
Full `cargo test --workspace` + `ledgerful verify` not re-proven in r3; `review.md` + conductor Completed left to orchestrator.

### [P3] Spec out-of-scope residuals (acceptable)
`D-0077-parallel-attrib` (0079), tracing-layer, desk-subscriber, gui drill-down, repair-diff — per spec §4.

### [P3] Missing hostile-folder / two-source attribution / workspace-grep vocabulary tests
Manual grep shows no competing risk enum; formal tests residual.

---

## Completeness sweep

| Area | Result |
|---|---|
| Placeholders / stubs | No TODO stubs in core CRC paths |
| Telemetry → report → export_risk → Desk | Wired |
| Attach-meta → CRC_SUSPECT (scan) | Wired |
| Attach-stream → IntegrityReason | **Wired** (materialize + probe) |
| Identity strip honesty | Pre-strip telemetry; dual-rate only; integrity.csv reconciled |
| Exit codes | Unchanged (correct) |
| `crc_skip_rate` meaning | Preserved + tested |
| Docs decision tree / SEC-06 / D-0074 & D-0073 closed | Present |

---

## Wiring diagram (post r2 fix)

```
NDB validate → note_* → TLS/global counters + emission gate
     ↓
message props/extract scope (block CRC+BID) → props/extract.crc_suspect
attach-meta (scan with_crc_scope) → msg_crc_suspect OR
attach open/stream → AttachmentDataReader.crc_suspect
     ↓ consumed by:
     materialize soft_reasons + probe outcome.reason=CrcSuspect
     probe_scan_items / peer probe push_degraded(CrcSuspect)  [not attach-fail rate]
     ↓
scan: IntegrityReason::CrcSuspect → Tier-2 block (unless allow flag)
     ↓
[if dual-rate poly_class] identity strip candidates/rows/tallies
     crc_suspect_messages stays pre-strip
     integrity.csv buffer retain(!CrcSuspect) then flush
     keep-set rebuild authoritative (streaming may under-merge)
     ↓
keep-set / fidelity tier 3 / export_risk (raw rates) / Desk banner
```

---

## Verification evidence

**Static (this review):** all r2 fix claims read and cross-checked against source; DoD matrix re-walked.

**Runtime:** not re-executed in r3 environment. Implementation-notes r2 fix-round claims retained:

```text
cargo fmt --all --check          OK (claimed)
cargo clippy --workspace --all-targets -- -D warnings   OK (claimed)
cargo test -p pst-reader         OK (claimed)
cargo test -p dedup-engine       OK (claimed)
cargo test -p pst-writer         OK (claimed)
cargo test -p pst-dedup-cli      OK (claimed; poly unit + sparse clean-twin + aspose 17/17 + crc_integrity_0077)
cargo test -p pst-dedup-gui      OK (claimed; unique_done_banner_mapping)
```

Release binary present: `target/release/pst-dedup.exe`.

**Orchestrator still owns:** full DoD-17 workspace gate + ledgerful verify evidence; DoD-18 `review.md` + conductor Completed.

---

## What is solid (keep)

1. Data-path CRC counters + bounded emission (D1/D2).
2. Per-source rates; `crc_skip_rate` meaning held.
3. Block-level `CRC_SUSPECT` for props/extract + attach-meta + attach-stream consumers; Tier-2 default block; MID Tier-1; allow-flag; graded fidelity arm — with proof tests including clean twin.
4. Dual-rate poly identity strip only (high block alone keeps taint); pre-strip telemetry; integrity.csv reconciled post-strip.
5. `export_risk` on `PreflightRecommendation`; monotone; advisory vs catastrophic; scan-preflight reasons.
6. ExportSection attach event totals on success; attach Vec cap 1000.
7. Desk banner no longer unqualified green success; GUI scan integrity parity.
8. Docs decision tree + SEC-06 observability + closed D-0074/D-0073.
9. CRC remains warning-only; no exit-code change; no second risk vocabulary.

---

## Completion decision

### Verdict: **PASS WITH DEFERRED P3**

### Why not FAIL
All r2 **P2** items are addressed:
1. Dual-rate poly gate replaces block-only strip; residual correctly **P3**.
2. integrity.csv buffered and flushed post-strip.
3. Attach payload `crc_suspect` consumed into message integrity on materialize and probe paths.
4. DoD-16 fixture-scale timing recorded; multi-GB is non-blocking residual.

### Why not full PASS
Documented **P3** residuals remain (systematic-poly fingerprint, DoD-16 multi-GB, process DoD-17/18, cancel ExportSection, BID assert, unique-pst CRC human line, page-taint policy). These do not block track completion under “only P3 residuals for non-FAIL.”

### Orchestrator next steps
1. Optional: re-run full workspace gate + `ledgerful verify` for DoD-17 evidence.
2. Write track `review.md`; flip conductor / sequencing to **Completed** (DoD-18).
3. Ledger commit for 0077 if not yet done.
4. Leave `D-0077-systematic-poly` and sibling D-0077-* rows as P3 residuals.

---

## Fix-round scorecard

| Area | r2 | r3 |
|---|---|---|
| Dual-rate poly strip | FAIL P2 (block-only) | **Met** (P3 residual) |
| integrity.csv reconcile | FAIL P2 | **Met** |
| Attach-stream → IntegrityReason | FAIL P2 | **Met** |
| DoD-16 timing | FAIL P2 | **Partial met** (fixture; multi-GB P3) |
| Sparse clean twin | P3 open | **Met** |
| D-0077-systematic-poly severity | P2 | **P3** |
| Overall | **FAIL** | **PASS WITH DEFERRED P3** |
