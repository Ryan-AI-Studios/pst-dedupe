# 0077 CRC_SUSPECT / integrity_telemetry / export_risk — Correctness Review (r1b)

**Branch:** `feat/0077-crc-noise-export-risk`  
**Scope:** read-only deep review of listed code paths  
**Date:** 2026-07-28  
**Reviewer:** subagent r1b  

---

## Verdict: **PASS WITH DEFERRED P3**

Primary data-path design is sound: CRC remains warning-only; counters are exact; emission is gated; message taint is **block-level** (not page-poly); sparse block CRC still taints; `export_risk` is wired into unique-pst `summary.json` and Desk banner; CLI flags exist on both clap surfaces. No P0. Residual P2 items should be tracked before calling the track fully closed; P3 deferred polish only.

---

## Special scrutiny (answered first)

### 1. Does the 50% systematic-CRC strip hide real sparse corruption?

**No.** Strip gates on **block** mismatches only:

```1079:1082:crates/pst-dedup-cli/src/scan.rs
        const SYSTEMATIC_BLOCK_CRC_RATE: f64 = 0.50;
        let block_read_denom = tel.block_reads.max(1) as f64;
        let systematic_crc =
            (tel.block_crc_mismatches as f64 / block_read_denom) >= SYSTEMATIC_BLOCK_CRC_RATE;
```

- Sparse real corruption (e.g. 0.1%–5% of block reads) stays **below** 0.50 → **taint kept**.
- Catastrophic medium (≥15% of *all* reads, including pages) still elevates `export_risk` via `block_crc_read_rate` even if message taint is stripped for poly-class stores.
- Strip zeros **reported** `crc_suspect_messages` and candidate/report integrity; **raw CRC counters remain**.

### 2. Can page-only CRC corruption poison Tier-2 without taint?

**Yes, by deliberate design.** Message scope uses **block CRC + BID only**:

```261:288:crates/pst-reader/src/integrity_telemetry.rs
/// Message-scope `CRC_SUSPECT` taint uses this rather than full mismatch total:
/// some real PSTs / fixtures use a non-standard *page* CRC polynomial ...
pub fn tls_block_mismatch_total() -> u64 { ... }
impl MessageScope {
    pub fn exit(self) -> bool {
        tls_block_mismatch_total() > self.start
    }
}
```

Page CRC failures never set `crc_suspect`. That preserves DoD-12 clean fixtures but means a true structural page failure that still returns coherent (CRC-valid) data blocks can feed Tier-2 without `CRC_SUSPECT`. Residual: inherent to warning-only page CRC + poly false-positive avoidance (P2 design residual, not a regression vs pre-0077).

### 3. Race / ordering of TLS counters vs multi-message scans

**Safe on current sequential single-thread scan path.**

| Concern | Assessment |
|---|---|
| Per-message enter/exit | Uses TLS block totals only; no mid-message `snapshot()` in scan loop |
| `snapshot()` / `flush_summary()` | Zeroes TLS after merge to globals; called **between sources**, not between messages |
| Cross-message bleed | Folder/open block CRCs before `message_scope_enter` raise the baseline; only **new** mismatches during the scope taint that message — correct |
| Mid-scope `snapshot()` | Would false-negative taint (TLS reset under an open scope). Not done today; **D-0077-parallel-attrib** must not flush TLS mid-message |
| Multi-source attribution | Sequential + end-of-source flush; deltas exact for counters. `distinct_bad_bids` is **not** a true per-source set (post-state global; documented in `delta_since`) |

### 4. Is `export_risk` actually wired into unique-pst report build?

**Yes.** Success path:

```2129:2161:crates/pst-dedup-cli/src/unique_pst_cmd.rs
    let export_risk = crate::unique_export_report::compute_export_risk(
        &outcome.summary.preflight.recommendation,
        &crate::unique_export_report::ExportRiskInputs {
            attach_fail_rate,
            block_crc_rate: outcome.summary.block_crc_rate,
            block_crc_read_rate: outcome.summary.block_crc_read_rate,
            degraded_winner_rate,
            partial: export_section.partial,
            failed_volume_index: export_section.failed_volume_index,
            scan_recommendation: outcome.summary.preflight.recommendation,
        },
    );
    let summary = UniqueExportSummary { ... export_risk, };
```

Also present on cancel path (`unique_pst_cmd.rs` ~763). Outcome surface carries `export_risk: summary.export_risk.level` for Desk.

### 5. Do human summary lines leak PST strings?

**New 0077 line does not.** Numbers/codes only:

```1757:1767:crates/pst-dedup-cli/src/main.rs
    // 0077: numbers only — no subjects/paths on new lines.
    println!(
        "  crc: page={} block={} bid={} distinct_bids={} exact={} suspect_msgs={} read_rate={:.4}",
        ...
    );
```

Telemetry warn lines use BIDs/hex only. Pre-existing summary fields (`skipped_by_reason`, paths elsewhere) are out of 0077 rule-7 scope (0081 residual).

### 6. Are both CLI parsers updated?

**Yes.**

| Surface | Flags |
|---|---|
| `main.rs` Commands: `Scan`, `Dups`, `KeepSet`, `UniqueEml` | `--allow-crc-suspect-tier2`, `--crc-log-limit`, `--crc-log-interval-secs` |
| `unique_pst_cmd::UniquePstClapArgs` (nested UniquePst parser) | same three, mapped in `into_cli_args` |
| Wiring | `apply_crc_log_limits` / `set_log_limit`; grouping gets `allow_crc_suspect_tier2` |

---

## Path-by-path findings

### 1. `integrity_telemetry.rs` — counters, TLS, flush, gate, BID cap

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| F-TEL-1 | — | Count-first, emit-second holds; mismatch_total includes page/block/BID | `note_mismatch` / `note_block_bid_mismatch` always increment before gate |
| F-TEL-2 | — | Distinct BID cap 1024 + `exact=false` when overflow | `record_bad_bid` + global merge in `flush_tls_to_global` ~336–345, 441–457 |
| F-TEL-3 | — | Emission: first-N detail → interval aggregate → `flush_summary` | `maybe_emit` / `maybe_aggregate` / `flush_summary` |
| F-TEL-4 | P3 | Poisoned `global_bad_bids` mutex → snapshot reports `(0, true)` (under-count + false exact) | `read_global_snapshot` ~463–466 `.unwrap_or((0, true))` |
| F-TEL-5 | P3 | Process-global state requires `TEST_LOCK`; production single-thread OK | module docs + tests |

### 2. `page.rs` + `block.rs` — CRC paths gated; reads counted

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| F-NDB-1 | — | Page CRC only via `validate` → `note_page_read` + `note_page_crc` | `page.rs:108–111` |
| F-NDB-2 | — | Block CRC/BID via `read_raw_block` → `note_block_read` + trailer validate | `block.rs:78–102` |
| F-NDB-3 | — | No remaining free-form CRC `tracing::warn!` outside telemetry gate | grep: only `integrity_telemetry.rs` |

### 3. `message.rs` — scope enter/exit; which counters count

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| F-MSG-1 | — | Both `read_message_properties_with_opts` and `read_message_extract` wrap scope | `message.rs:207–267`, `292–334` |
| F-MSG-2 | P2 | Doc comments claim “page/block CRC”; runtime is **block+BID only** | `message.rs:62–64`, `117–118` vs `tls_block_mismatch_total` |
| F-MSG-3 | P2 | Spec §3.3a also named attachment stream scopes; attach meta/stream are **outside** message scope in scan | `scan.rs:704` then later `764` `read_attachment_metadata` after props return |

Attachment-path CRC can still inflate source counters and rates without setting `props.crc_suspect`.

### 4. `scan.rs` — attribution, rates, systematic strip, crc_suspect, crc_skip_rate

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| F-SCN-1 | — | Per-source snapshot before open; end flush + delta | `scan.rs:538–539`, `1071–1073` |
| F-SCN-2 | — | `crc_skip_rate` still message-level skips only (preflight) | preflight inputs `crc_skips`; test `crc_integrity_0077.rs:143–147` |
| F-SCN-3 | — | Rates: `block_crc_rate` and clamped `block_crc_read_rate` | `scan.rs:1526–1529` |
| F-SCN-4 | — | Sparse block CRC still taints; systematic only at ≥50% **block** rate | `scan.rs:1079–1082` |
| F-SCN-5 | P2 | Systematic strip updates candidates / report rows / tallies / `crc_suspect_messages`, but **not**: (a) already-streamed `integrity.csv` degraded rows, (b) streaming `DedupIndex` inserts (`tier2_eligible` already false), (c) `tier2_blocked_crc_suspect` stats | `scan.rs:1084–1132` + comment 1130–1131 |
| F-SCN-6 | P2 | Per-file `distinct_bad_bids` is global post-state when any mismatch activity, not per-source distinct | `IntegritySnapshot::delta_since` `integrity_telemetry.rs:66–74` |
| F-SCN-7 | P3 | D-0077-parallel-attrib comment present at snapshot site | `scan.rs:523–526` |

**Keep-set / unique-pst:** rebuild from stripped candidates → eligibility correct after strip.  
**Streaming scan/dups unique counts:** may under-merge on poly-class block noise until strip (documented residual).

### 5. dedup-engine — Tier-2, fidelity, allow flag

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| F-ENG-1 | — | `CrcSuspect` distinct from `CrcMismatch`; serde string `CRC_SUSPECT` | `integrity.rs:110`, `153`, `218` |
| F-ENG-2 | — | Keep-set `assess_tier2_eligibility` blocks CrcSuspect first | `keepset.rs:481–489` |
| F-ENG-3 | — | `allow_crc_suspect_tier2` default false; resolve path honors it | `grouping.rs:215–258`, `keepset.rs:867–873` |
| F-ENG-4 | — | Graded fidelity: `CrcSuspect` → tier 3; clean beats suspect | `keepset.rs:1314–1315`, test ~4467 |
| F-ENG-5 | — | Streaming index path also gates on `props.crc_suspect` | `scan.rs:953–958`, GUI `worker.rs:292–296` |

### 6. `unique_export_report.rs` — export_risk composition / thresholds

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| F-XR-1 | — | Vocabulary is `PreflightRecommendation` (no low\|elevated\|high) | `ExportRisk.level` |
| F-XR-2 | — | Thresholds match spec: adv 0.05/0.01/0.02; cat 0.15/0.50 | `ExportRiskThresholds::default` ~192–200 |
| F-XR-3 | — | Monotone `scan.max(post)`; catastrophic rates alone → not_export_ready | `compute_export_risk_with_thresholds` ~238–324; unit tests ~1316–1372 |
| F-XR-4 | P3 | When scan is already `re_export_recommended` and post is `ok`, `reasons` may be **empty** (only `not_export_ready` scan is named) | ~256–258 only names not_export_ready |
| F-XR-5 | P3 | `partial` alone never raises risk (only with `failed_volume_index` for reason text) | ~246–254 — likely intentional |

### 7. `pst-writer` production — attach event cap

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| F-WR-1 | — | Cap 1000; total always increments; truncated flag; sink receives **all** events before Vec drop | `production.rs:661–717` |
| F-WR-2 | P2 | CLI `ExportSection.attachment_fidelity_events_{truncated,total}` always left `None` — writer report fields never copied into unique-export summary | `unique_pst_cmd.rs:2111–2112` |

Memory bound is real; report surface for the cap is incomplete.

### 8. GUI banner mapping

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| F-GUI-1 | — | Green only when `ok && export_risk == Ok`; yellow/red for risk; cancel/error separate | `unique_worker.rs:113–125`, `views/unique_wizard.rs:374–393` |
| F-GUI-2 | — | Unit test `unique_done_banner_mapping` covers matrix | `unique_worker.rs:352–380` |
| F-GUI-3 | P2 | Legacy GUI **scan** worker always stores `RecoverableIntegrity::clean()` even when `props.crc_suspect` (Tier-2 still blocked) | `worker.rs:318–321` vs `292–296` |

Unique wizard uses CLI unique-pst path (full scan integrity); legacy GUI scan CSV honesty is residual.

### 9. Tests `crc_integrity_0077.rs`

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| F-TST-1 | — | Synthetic corrupt fixture generate-at-test-time; counters + reads asserted | tests ~71–116 |
| F-TST-2 | — | `crc_skip_rate` meaning pin | ~143–147 |
| F-TST-3 | P3 | No assertion that a recoverable message sets `CRC_SUSPECT` / `crc_suspect_messages > 0` under controlled sparse block flip | file covers counters/rates/golden counts only |
| F-TST-4 | — | Telemetry tests use `TEST_LOCK` | integration + unit |

---

## Finding severity summary

### P0
*None.*

### P1
*None blocking merge of the primary CRC_SUSPECT / export_risk design.*  
(If track closure requires DoD-11 report fields and full strip reconciliation, treat F-WR-2 and F-SCN-5 as P1 before “Completed”.)

### P2 (should track / fix before claiming full DoD)

1. **F-SCN-5** — Systematic strip incomplete: `integrity.csv` + streaming index eligibility/`tier2_blocked_crc_suspect` not reconciled (`scan.rs:1084–1132`).
2. **F-WR-2** — `ExportSection.attachment_fidelity_events_*` never populated (`unique_pst_cmd.rs:2111–2112`).
3. **F-MSG-2 / F-MSG-3** — Doc/spec vs impl: block-only taint; attach reads outside scope (`message.rs:62–64`, `scan.rs:704` vs `764`).
4. **F-GUI-3** — GUI scan report integrity always clean (`worker.rs:318–321`).
5. **F-SCN-6** — Per-file `distinct_bad_bids` not true per-source distinct (`integrity_telemetry.rs:66–74`).
6. **Page-only structural risk** — Tier-2 not tainted by page CRC (intentional poly policy).

### P3 (deferred polish)

1. **F-XR-4** — Empty `export_risk.reasons` when only scan elevates to re_export.
2. **F-TEL-4** — Mutex poison fallback under-counts distinct BIDs.
3. **F-TST-3** — No integration assert for message-level `CRC_SUSPECT` on sparse synthetic corruption.
4. **F-SCN-7 / parallel** — Mid-message TLS flush would break taint (0079 constraint).
5. Doc drift: message.rs “page/block” wording.

---

## What is correct (strengths)

1. **D2 fixed:** page/block CRC volume is counted in the data path independent of tracing subscribers.
2. **D7 fixed (block data):** recovered-from block CRC taints items, blocks Tier-2 by default, graded fidelity ranks clean above suspect.
3. **Noise control:** first-N + interval aggregate + flush; totals exact.
4. **export_risk:** single vocabulary, monotone composition, advisory vs catastrophic thresholds, wired to summary + Desk.
5. **crc_skip_rate** meaning preserved.
6. **Attach Vec cap** prevents unbounded memory under fail storms (sink path still complete).
7. **CLI both parsers** carry 0077 flags.
8. **Systematic strip policy** is a reasonable poly-class detector that does **not** suppress sparse block taint; catastrophic rates still surface via `export_risk`.

---

## Recommended follow-ups (non-blocking for this review)

1. After systematic strip: suppress or rewrite integrity.csv CRC_SUSPECT rows for that source; optionally re-open Tier-2 eligibility for streaming stats (or document that only keep-set is authoritative).
2. Aggregate `report.attachment_fidelity_events_truncated/total` into `ExportSection` across volumes.
3. Optionally wrap `read_attachment_metadata` / attach stream in `MessageScope` (or union scopes) if attach-block CRC should taint identity.
4. Align `message.rs` docs with block-only taint; note residual in spec decision table.
5. GUI scan path: merge `CrcSuspect` into `ReportRow.integrity` when `props.crc_suspect`.
6. Integration test: one message, one block byte flip, assert `crc_suspect_messages ≥ 1` and Tier-2 block without systematic strip.

---

## Verdict rationale

Core 0077 safety properties for unique-pst / keep-set hold under sequential scan. Special scrutiny items resolve without P0/P1 defects on the golden export path. Remaining issues are reconciliation/reporting residuals (P2) and polish (P3) — **PASS WITH DEFERRED P3**, with P2 listed for orchestrator tracking before Conductor “Completed”.
