# Track Completion Audit r2 — 0077-CrcNoiseAndExportRisk

**Branch:** `feat/0077-crc-noise-export-risk`  
**Date:** 2026-07-28  
**Reviewer:** subagent r2 (read-only re-review after fix round)  
**Prior:** `review.subagent-r1.md` (FAIL), `review.subagent-r1b.md` (PASS WITH DEFERRED P3 — correctness path), fix round in `implementation-notes.md`

---

## Verdict: **FAIL**

Fix round F1–F7 **closes the prior r1 P1 wiring/test gaps** that were blocking D7/DoD-19/20 proof (attach-meta scope, sparse taint E2E, DoD-20 units, ExportSection totals, GUI integrity, export_risk reasons, pre-strip telemetry). That is real progress.

Remaining **P2** residuals still fail the completion bar (only P3 may remain for PASS WITH DEFERRED P3):

1. **Systematic identity strip at ≥50% block-CRC rate** still clears `CRC_SUSPECT` from candidates/rows (rule 10 / DoD-12 split-only / D7) — now documented as `D-0077-systematic-poly` (P2), not removed or narrowed to a poly allowlist.
2. **Strip does not reconcile `integrity.csv`** (or streaming index / `tier2_blocked_crc_suspect` stats) — ledger vs keep-set identity diverge under systematic strip.
3. **Attach *payload* stream `crc_suspect` is instrumented but not consumed** into message `IntegrityReason` / materialize fidelity (scan attach-meta is fixed).
4. **DoD-16** page-heavy before/after timing still unrecorded.

Process DoD-17/18 also remain incomplete for track close (orchestrator owns conductor/`review.md`).

---

## Scope Reviewed

| Artifact | Path |
|---|---|
| Spec / notes / prior reviews | `conductor/0077-CrcNoiseAndExportRisk/{spec,implementation-notes,review.subagent-r1,review.subagent-r1b}.md` |
| Telemetry | `crates/pst-reader/src/integrity_telemetry.rs` |
| NDB | `crates/pst-reader/src/ndb/{page,block}.rs` |
| Message / attachment | `crates/pst-reader/src/messaging/{message,attachment}.rs` |
| Scan + strip | `crates/pst-dedup-cli/src/scan.rs` |
| Export risk / unique-pst | `crates/pst-dedup-cli/src/{unique_export_report,unique_pst_cmd}.rs` |
| Materialize / attach probe | `crates/pst-dedup-cli/src/{pst_materializer,attach_probe}.rs` |
| Keep-set / fidelity | `crates/dedup-engine/src/keepset.rs` |
| Writer cap | `crates/pst-writer/src/production.rs` |
| GUI | `crates/pst-dedup-gui/src/{worker,unique_worker,views/unique_wizard}.rs` |
| Tests | `crates/pst-dedup-cli/tests/crc_integrity_0077.rs`, keepset units, export_risk units |
| Docs | `docs/{unique-pst-export,audit,deferred}.md` |

---

## Fix-round claim verification (F1–F7)

| Claim | Status | Evidence |
|---|---|---|
| **F1** attach stream/meta CRC taint | **Partial → residual P2** | `with_crc_scope` `integrity_telemetry.rs:298-305`; scan attach-meta `scan.rs:758-768`; `list_attachments` scope `attachment.rs:164-171`; `open_attachment_data` scope + flag `attachment.rs:247-265`; leaf stream sets flag `attachment.rs:104-110`. **Gap:** `pst_materializer.rs:368-383` and `attach_probe.rs:706+` never call `reader.crc_suspect()` or OR into `IntegrityReason::CrcSuspect`. |
| **F2** ExportSection attach totals | **Met (success path)** | Aggregate `unique_pst_cmd.rs:1829-1832`; surface `2118-2119`. Cancel early path still `None` (`801-802`) — P3. |
| **F3** sparse taint + DoD-20 tests | **Met (partial proof)** | Integration `sparse_block_flip_taints_message_crc_suspect` `crc_integrity_0077.rs:156-223` (suspect count + candidate taint; rate &lt; 0.50). DoD-20 units: `keepset.rs:4486-4577` (ineligible, MID groups, allow-flag restores Tier-2). **Gap:** same-file clean twin **not** asserted untainted (verification #7 half-open) → P3. |
| **F4** unique-pst `export_risk` human line | **Met** | `unique_pst_cmd.rs:2278-2279` codes-only. Spec §3.10 also asked CRC counts on unique-pst human summary — only `export_risk` line added (CRC remains scan human line `main.rs:1757-1767`) → P3 residual. |
| **F5** pre-strip `crc_suspect_messages` + defer strip | **Partial** | Pre-strip telemetry `scan.rs:1084-1098`, `1175`; identity strip still at `1092-1148`. Residual `docs/deferred.md:814` **D-0077-systematic-poly P2**. |
| **F6** GUI scan `CRC_SUSPECT` | **Met** | `worker.rs:318-326` builds degraded integrity when `props.crc_suspect`. |
| **F7** `scan_preflight` reason | **Met** | `unique_export_report.rs:259-262`; unit assert `1333-1340`. |

---

## Prior P1 disposition (r1)

### P1-1 Systematic 50% strip undoes CRC_SUSPECT — **still open as P2**

**Was:** P1 — strip zeros identity + reported count; re-opens Tier-2 on high block-CRC stores.

**After fix:**
- `crc_suspect_messages` is **pre-strip** (`scan.rs:1097-1098`, `1175`) — honesty fix holds.
- Raw counters/rates unchanged; `export_risk` still sees rates.
- **Identity strip still runs** when `block_crc_mismatches / block_reads ≥ 0.50` (`scan.rs:1092-1116`), clearing `CrcSuspect` from candidates/rows so keep-set rebuild can re-merge Tier-2.

**Not in original spec.** Notes justify aspose poly; page CRC is already excluded from taint (`integrity_telemetry.rs:261-268`). Block-rate strip remains over-broad for real high-rate block corruption.

**Severity now:** **P2** (not P1) — telemetry no longer lies; D7 identity still defeated at ≥50% block rate; explicit residual row exists. Still **blocks PASS WITH DEFERRED P3**.

**Risk window vs catastrophic export_risk:** strip uses **block-only** rate 0.50; catastrophic keys on **`block_crc_read_rate`** (page+block) at 0.15. A store can hit strip while total read rate stays below catastrophic (page-heavy clean denominator), so operators may only see `re_export_recommended` while identity taint is cleared.

### P1-2 Attachment streams not in message scope — **mostly fixed; residual P2**

**Was:** no scope on attach paths; scan attach-meta after props exit.

**After fix:** scopes on list/open/stream; scan ORs attach-meta into `msg_crc_suspect` (`scan.rs:758-829`).

**Residual P2:** payload-stream flag not consumed by materialize/probe into message integrity (see F1). Scan-time Tier-2 for attach-meta is fixed; attach-payload-only corruption at export still lacks `CRC_SUSPECT` on fidelity.

### P1-3 No scoped taint integration test — **fixed (P3 remainder)**

**Was:** no E2E proof.

**After fix:** `sparse_block_flip_taints_message_crc_suspect` proves sparse body-block flip → `crc_suspect_messages > 0` + candidate `CrcSuspect`, with rate &lt; systematic threshold.

**P3:** does not assert the clean sibling in the same file is untainted.

---

## Prior P2 disposition

| Prior finding | After fix round |
|---|---|
| DoD-20 unproven | **Met** — keepset units MID + allow-flag |
| DoD-11 ExportSection totals always None | **Met** on success path; cancel path None → P3 |
| DoD-10 weak (sum only) | **Improved** — page and block classes asserted separately (`crc_integrity_0077.rs:135-145`); BID class not asserted → P3 |
| Page CRC excluded from taint | **Intentional residual** — documented in telemetry/message docs; poly safety for DoD-12. Full §3.3a over-inclusive page taint not shipped → **P3** design residual (not silent bug) |
| DoD-16 / 17 / 18 process | **Unmet** — no timing record; full gate not re-run in r2; no `review.md` / conductor Completed |
| r1b F-SCN-5 strip incomplete (csv/index/stats) | **Still open P2** — strip mutates candidates/rows/tallies only; integrity.csv already streamed (`scan.rs:841-862`); streaming index eligibility not rebuilt; comment admits under-merge (`1145-1147`) |
| r1b F-GUI-3 | **Met** |
| r1b F-WR-2 | **Met** (success) |
| r1b F-XR-4 empty reasons | **Met** (F7) |

---

## DoD matrix (r2)

| DoD | Status | Notes |
|---|---|---|
| **1** telemetry module | **Met** | TLS + global + BID cap + snapshot/delta/reset/set_log_limit/flush_summary |
| **2** warn sites gated | **Met** | `page.rs:108-112`, `block.rs:79-102`; CRC warns only in `integrity_telemetry` |
| **3** bounded emission | **Met** | unit `bounded_emission_with_exact_total` |
| **4** scan counters + serde default | **Met** | FileScanStats / ScanSummary fields |
| **5** rates + crc_skip_rate | **Met** | integration pin `scan_reports_crc_fields_and_crc_skip_rate_unchanged` |
| **19** CRC_SUSPECT scopes | **Partial** | props/extract + scan attach-meta met; attach stream flag set but not always applied to item integrity; clean-twin precision test incomplete |
| **20** Tier-2 / MID / allow | **Met** | logic + units |
| **21** fidelity arm | **Met** | `reason_fidelity_tier` + clean &gt; suspect |
| **22** crc_suspect_messages | **Met** | per-file + total; pre-strip honesty; human scan line |
| **6** export_risk vocabulary | **Met** | `PreflightRecommendation` only |
| **7** monotone composition | **Met** | unit + F7 reasons |
| **8** advisory vs catastrophic | **Met** | units 0.06 attach / 0.20 read rate |
| **23** Desk banner | **Met** | green only when risk Ok; unit mapping |
| **9** CLI flags both parsers | **Met** | scan/dups/keep-set/unique-eml/unique-pst |
| **10** synthetic corrupt fixture | **Mostly met** | page+block asserted; BID class soft |
| **11** attach event cap + surface | **Met** | cap 1000 + ExportSection totals on success |
| **12** clean corpus / split-only | **Partial** | aspose 17/17; systematic strip is **merge-enabling** on high block-rate sources (violates split-only spirit of DoD-12 corrupt case) |
| **13** numbers-only new lines | **Met** | scan CRC + unique-pst export_risk |
| **14** decision tree docs | **Met** | `docs/unique-pst-export.md` |
| **15** SEC-06 | **Met** | `docs/audit.md` counted per source |
| **16** perf timing | **Unmet** | residual |
| **17** full gate | **Unverified in r2** | notes claim targeted packages green; workspace + ledgerful not evidenced here |
| **18** review.md + conductor Completed | **Unmet** | orchestrator |

### Locked rules (§2.3)

| Rule | Status |
|---|---|
| Count first, log second | Met |
| Counters in data path | Met |
| One risk vocabulary | Met |
| CRC warning-only | Met |
| Bounded memory | Met (BID set + attach Vec) |
| Sources read-only | Met |
| New lines counters/codes only | Met |
| Additive JSON / serde default | Met |
| No exit-code change | Met |
| Recovered corruption still corruption | **Violated for identity at ≥50% block rate** (systematic strip) |

---

## Findings (r2)

### [P2] Systematic identity strip still clears CRC_SUSPECT (D-0077-systematic-poly)
**Confidence:** High  

**Where:** `crates/pst-dedup-cli/src/scan.rs:1080-1148`  
**Deferred:** `docs/deferred.md:814` severity **P2**

Identity strip remains rate-only (`block_crc / block_reads ≥ 0.50`), not a poly allowlist. Pre-strip telemetry is honest; D7 identity protection is not. Keep-set rebuild from stripped candidates can re-enable Tier-2 merges over bytes that failed CRC during read.

**Required for upgrade:** remove strip, or gate on explicit poly signal (never clear taint solely because failure rate is high); never merge-increase on high-rate real corruption.

---

### [P2] Systematic strip does not reconcile integrity.csv / streaming index / block stats
**Confidence:** High  

**Where:** integrity rows written at `scan.rs:841-862` before strip at `1099+`; strip does not rewrite CSV or reopen `tier2_eligible` on the live index (comment `1145-1147`).

Operator surfaces disagree: integrity.csv says `CRC_SUSPECT`, candidates/keep-set may not, streaming unique counts may under-merge until rebuild.

---

### [P2] Attach payload stream `crc_suspect` not applied to message integrity consumers
**Confidence:** High  

**Where:** flag set `attachment.rs:104-110`, `247-265`, accessor `133-135`; **no** use in `pst_materializer.rs:368-383`, `attach_probe.rs:706+` (grep: only reader + scan props path).

DoD-19 names attachment stream reads as a taint site. Scan attach-meta is fixed; payload path is instrumented-but-orphaned for `IntegrityReason`.

---

### [P2] DoD-16 performance timing unrecorded
**Confidence:** High  

Notes and prior reviews admit residual; no before/after timing in review artifacts.

---

### [P3] Sparse taint test does not prove clean sibling untainted
**Where:** `crc_integrity_0077.rs:156-223` — two messages, asserts any suspect, not “exactly first tainted / second clean”.

### [P3] DoD-10 BID-mismatch class not asserted
Page + block asserted; BID not required in test.

### [P3] unique-pst human summary omits CRC counter line
Only `export_risk` level (`unique_pst_cmd.rs:2279`); scan has full CRC line.

### [P3] Cancel-path ExportSection attach event fields remain None
`unique_pst_cmd.rs:801-802`.

### [P3] Missing hostile-folder / two-source attribution / workspace-grep vocabulary tests
As r1 P3s; manual grep shows no competing risk enum.

### [P3] Spec out-of-scope residuals (acceptable)
`D-0077-parallel-attrib` (0079), tracing-layer, desk-subscriber, gui drill-down, repair-diff — per spec §4.

### [P3] Page-CRC exclusion from message taint
Documented deliberate poly policy; residual real page-structure risk without taint.

---

## Completeness sweep

| Area | Result |
|---|---|
| Placeholders / stubs | No TODO stubs in core paths; cancel ExportSection fields still None |
| Telemetry → report → export_risk → Desk | Wired |
| Attach-meta → CRC_SUSPECT (scan) | Wired |
| Attach-stream → IntegrityReason | **Not fully wired** |
| Identity strip honesty | Telemetry yes; identity policy residual P2 |
| Exit codes | Unchanged (correct) |
| `crc_skip_rate` meaning | Preserved + tested |
| Docs decision tree / SEC-06 / deferred closes D-0074/D-0073 | Present |

---

## Wiring diagram (post-fix)

```
NDB validate → note_* → TLS/global counters + emission gate
     ↓
message props/extract scope (block CRC+BID) → props.crc_suspect
attach-meta (scan with_crc_scope) → msg_crc_suspect OR
attach open/stream → AttachmentDataReader.crc_suspect  [not consumed by materialize]
     ↓
scan: IntegrityReason::CrcSuspect → Tier-2 block (unless allow flag)
     ↓
[if block_crc/block_reads ≥ 0.5] identity strip candidates/rows  ← residual P2
     crc_suspect_messages stays pre-strip
     integrity.csv already written with CRC_SUSPECT
     ↓
keep-set / fidelity tier 3 / export_risk (raw rates) / Desk banner
```

---

## Verification evidence

**Static (this review):** code paths above read and cross-checked against DoD and F1–F7 claims.

**Runtime:** not re-executed in r2 (read-only static re-review). Implementation-notes fix-round claims:

```text
cargo fmt --all --check          OK (claimed)
cargo clippy --workspace --all-targets -- -D warnings   OK (claimed)
cargo test -p pst-reader         OK (claimed)
cargo test -p dedup-engine       OK (claimed; DoD-20 units)
cargo test -p pst-writer         OK (claimed)
cargo test -p pst-dedup-cli      OK (claimed; crc_integrity_0077 sparse + per-class)
cargo check/test -p pst-dedup-gui OK (claimed; unique_done_banner_mapping)
```

**DoD-17 gap:** full `cargo test --workspace` + `ledgerful verify` not evidenced after fix round in this review.

---

## What is solid (keep)

1. Data-path CRC counters + bounded emission (D1/D2).
2. Per-source rates; `crc_skip_rate` meaning held.
3. Block-level `CRC_SUSPECT` for props/extract + scan attach-meta; Tier-2 default block; MID Tier-1; allow-flag; graded fidelity arm — **with proof tests**.
4. `export_risk` on `PreflightRecommendation`; monotone; advisory vs catastrophic; scan-preflight reasons.
5. ExportSection attach event totals on success; attach Vec cap.
6. Desk banner no longer unqualified green success; GUI scan integrity parity.
7. Docs decision tree + SEC-06 observability + closed D-0074/D-0073.
8. CRC remains warning-only; no exit-code change; no second risk vocabulary.

---

## Completion decision

### Verdict: **FAIL**

### Why not PASS / PASS WITH DEFERRED P3
- Residual **P2** systematic identity strip (`D-0077-systematic-poly`) still undoes D7 on high block-CRC-rate stores (rule 10 / DoD-12).
- Residual **P2** strip vs integrity.csv / streaming index reconciliation.
- Residual **P2** attach payload stream taint not applied to message integrity consumers.
- Residual **P2** DoD-16 timing.

P3-only residuals would allow PASS WITH DEFERRED P3; the above do not.

### Minimum to re-grade PASS WITH DEFERRED P3
1. **Resolve systematic strip** so high real block-CRC rate never clears identity taint (remove, or poly allowlist only); keep pre-strip telemetry.
2. If strip remains for any reason: reconcile integrity.csv + document streaming-index authority; or rewrite ledger for that source.
3. OR attach-stream `crc_suspect` into materialize/probe fidelity (or document attach-payload as export-ledger-only residual demoted with product sign-off).
4. Record DoD-16 fixture-scale timing (even if multi-GB deferred as P3).
5. Orchestrator: DoD-17 gate evidence + DoD-18 `review.md` / conductor Completed.

### Optional P3 polish (non-blocking once P2s cleared)
- Clean-twin untainted assert; BID class assert; unique-pst CRC human line; cancel ExportSection zeros; hostile-folder / two-source tests.

---

## Fix-round scorecard

| Area | r1 | r2 |
|---|---|---|
| Attach-meta taint | FAIL P1 | **Met** |
| Attach-stream instrumentation | FAIL P1 | Met API; consumer residual P2 |
| Sparse taint E2E | FAIL P1 | **Met** (+ P3 clean twin) |
| DoD-20 tests | FAIL P2 | **Met** |
| ExportSection totals | FAIL P2 | **Met** (success) |
| export_risk human line | FAIL P3 | **Met** |
| crc_suspect_messages honesty | FAIL (zeroed) | **Met** pre-strip |
| Systematic identity strip | FAIL P1 | **Still P2** |
| GUI scan integrity | FAIL P2 | **Met** |
| scan_preflight reasons | FAIL P3 | **Met** |
| Overall | **FAIL** | **FAIL** (narrower residuals) |
