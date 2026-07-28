# 0077 Implementation Notes — CRC Noise Control & Export Risk

**Branch:** `feat/0077-crc-noise-export-risk`  
**Ledger:** pending tx (not committed by implementer)  
**Conductor status:** left for orchestrator (not flipped to Completed)

## What shipped

### Phase 0 — Baseline
- `baseline.md` + JSON captures for `aspose_outlook.pst` (17 msg / 17 unique) and `promotions_spam.pst` (0 msg walker)
- `tracing` **0.1.44** / `tracing-subscriber` **0.3.23** recorded

### Phase 1–2 — `integrity_telemetry` + warn routing
- New `crates/pst-reader/src/integrity_telemetry.rs`: TLS `Cell` counters, global atomics, capped distinct BIDs (1024 + exact flag), emission gate (first-N + interval aggregate + flush_summary)
- Wired `page.rs` / `block.rs` through `note_page_crc` / `note_block_crc` / `note_block_bid_mismatch` + read counters
- No remaining CRC `tracing::warn!` outside the gate

### Phase 3 — Synthetic corrupt fixture
- `crates/pst-dedup-cli/tests/crc_integrity_0077.rs`: generate via `pst-writer`, flip bytes, assert counters
- Never commits a real-file-derived corrupt PST

### Phase 3a — `CRC_SUSPECT`
- `IntegrityReason::CrcSuspect` → `"CRC_SUSPECT"` (distinct from `CrcMismatch`)
- Message-scope enter/exit on `read_message_properties` / `read_message_extract` using **block** CRC+BID delta (page CRC alone does not taint — poly false-positive class)
- Scan merges taint into `RecoverableIntegrity`; Tier-2 ineligible unless `--allow-crc-suspect-tier2`
- `reason_fidelity_tier` arm = graded tier 3; clean beats suspect (DoD-21 test)
- **Systematic CRC identity strip:** if `block_crc / block_reads ≥ 0.50` for a source (aspose non-standard poly), strip `CRC_SUSPECT` from candidates/rows for DoD-12 while **keeping raw CRC counters** and **pre-strip `crc_suspect_messages` telemetry**

### Phase 4 — Scan wiring
- Per-source snapshot/delta → `FileScanStats` fields (`#[serde(default)]`)
- `ScanSummary` totals + `distinct_bad_bids_exact` + `block_crc_rate` + `block_crc_read_rate ∈ [0,1]`
- `crc_skip_rate` meaning unchanged (message skips only)
- Comment `D-0077-parallel-attrib` at snapshot site

### Phase 5 — `export_risk`
- `ExportRisk { level: PreflightRecommendation, reasons, inputs, thresholds }` on `UniqueExportSummary`
- Monotone max(scan, post-export); advisory vs catastrophic tiers per spec
- Unit tests: monotone, advisory 0.06 attach fail, catastrophic 0.20 read rate

### Phase 6 — Attach event cap
- `ATTACHMENT_FIDELITY_EVENTS_CAP = 1000` + `_total` + `_truncated` on `WritePstReport`

### Phase 7 — CLI + Desk
- `--crc-log-limit` (default 10), `--crc-log-interval-secs` (default 30), `--allow-crc-suspect-tier2` on scan/dups/keep-set/unique-eml/unique-pst (both parsers)
- Human scan summary CRC line (numbers only)
- Desk: `UniqueOutcomeView.export_risk`; banner only green when `ok` **and** risk `Ok`; yellow/red otherwise; stats row; unit test `unique_done_banner_mapping`

### Phase 9 — Docs
- `docs/unique-pst-export.md` integrity decision tree (ScanPST / Purview / this tool)
- `docs/audit.md` SEC-06 → counted per source (not closed)
- `docs/deferred.md`: D-0074-crc-fixture + D-0073-vec-events **closed / 0077**; D-0077-* residuals added

## Fix round (review r1 / r1b) — 2026-07-28

Addressed blocking and easy review findings without exit-code or vocabulary changes.

### F1 — Attach stream / attach-meta CRC taint (DoD-19)
- `integrity_telemetry::with_crc_scope` helper ORs block CRC/BID delta into a caller flag
- `list_attachments` / `open_attachment_data` run under message scope; `AttachmentDataReader` tracks `crc_suspect` on open + leaf stream reads
- Scan wraps attach-meta probe with `with_crc_scope` and merges into message `CRC_SUSPECT`
- Docs: taint is **block CRC + BID only** (page deliberately excluded for poly-class fixtures)

### F2 — ExportSection attach event totals (DoD-11)
- unique-pst success path aggregates `report.attachment_fidelity_events_{total,truncated}` into `ExportSection` (no longer always `None`)

### F3 — DoD-19/20 proof tests
- Integration: `sparse_block_flip_taints_message_crc_suspect` — flip one message block **trailer CRC** only (data intact), assert `crc_suspect_messages > 0`, rate << 0.5, candidate identity taint
- Integration: DoD-10 strengthened — assert `page_crc_mismatches > 0` **and** `block_crc_mismatches > 0` separately on mass synthetic flips
- Unit (keepset): Tier-2 ineligible by default; MID still groups suspect+clean twins; `--allow-crc-suspect-tier2` restores Tier-2 merge

### F4 — unique-pst human `export_risk` line (DoD-13)
- Numbers/codes only: `export_risk: <level.as_str()>` after partial/ok line

### F5 — Systematic strip honesty (P1 partial)
- **Identity strip kept** for DoD-12 aspose poly (≥0.50 block rate)
- `crc_suspect_messages` is now **pre-strip** hit count (honest “messages that saw block CRC during read”)
- Comments at strip site: raw counters + `export_risk` rates unchanged; catastrophic ≥0.15 still `not_export_ready`
- Residual `D-0077-systematic-poly` added to `docs/deferred.md`

### F6 — GUI scan integrity honesty (F-GUI-3)
- Legacy GUI scan worker stores `RecoverableIntegrity` with `CRC_SUSPECT` when `props.crc_suspect` (CLI parity)

### F7 — export_risk reasons when only scan elevates
- `scan_preflight=re_export_recommended` added to reasons when scan is already re_export (not only not_export_ready)

## Fix round r2 (review.subagent-r2 FAIL residuals) — 2026-07-28

Closed remaining **P2** items without exit-code / vocabulary / risk-enum changes.

### P2-1 — Poly-class dual-rate identity strip
- Replaced block-only `block_crc/block_reads ≥ 0.50` gate with **dual-rate** poly-class:
  ```
  poly_class = (block_crc/block_reads ≥ 0.50) AND (page_crc/page_reads ≥ 0.50)
  ```
- Helper `is_poly_class_crc(page_crc, page_reads, block_crc, block_reads) -> bool` in `scan.rs`
- High block + low page → **keeps** `CRC_SUSPECT` identity (real data-block corruption)
- Unit: `poly_class_requires_dual_high_rate`
- Aspose DoD-12 golden still 17/17 (hits both page and block poly)
- `docs/deferred.md` `D-0077-systematic-poly` demoted **P2 → P3**; documents dual-rate policy

### P2-2 — integrity.csv reconciled after strip
- Degraded integrity rows buffered **per-file** (`file_integrity_degraded: Vec<SkipRecord>`)
- Flushed after poly strip decision; strip filters `CRC_SUSPECT` from buffer so integrity.csv matches keep-set identity
- Skip rows still stream immediately (crash resilience)
- Streaming index: keep-set rebuild remains authoritative; poly under-merge until rebuild documented in comments

### P2-3 — Attach payload stream `crc_suspect` consumed
- `pst_materializer`: extract/props `crc_suspect` → message `CRC_SUSPECT`; after `open_attachment_data` / `read_to_end`, `reader.crc_suspect()` ORs into soft_reasons; deep-probe + cache paths apply `CrcSuspect` when `ok && reason == CrcSuspect`
- `attach_probe::probe_attach_stream`: after open/head/full success, surface `reason: Some(CrcSuspect)` when reader flagged (still `ok: true`, warning-only)
- `probe_scan_items` / keep-set peer probe: `push_degraded(CrcSuspect)` for non-fail CRC reason (not attach-fail rate)

### P2-4 — DoD-16 fixture-scale timing
- Post-0077 wall time (release binary, no rebuild):
  ```
  target\release\pst-dedup.exe scan fixtures/aspose_outlook.pst --json
  wall ≈ 0.039 s (TotalMilliseconds 38.5)
  ```
- Pre-0077 not measured in same session → multi-GB ceiling not proven; residual operator for large stores
- DoD-16 demoted to residual/partial evidence (fixture-scale only)

### P3 polish — clean twin (sparse test)
- `sparse_block_flip_taints_message_crc_suspect`: when ≥2 candidates, assert at least one clean sibling without `CRC_SUSPECT` (not all-message taint)

## DoD coverage map

| DoD | Status |
|---|---|
| 1 telemetry module | met |
| 2 warn routing | met |
| 3 bounded emission | met (unit test) |
| 4 scan counters serde default | met |
| 5 rates + crc_skip_rate pin | met (integration test) |
| 6 export_risk PreflightRecommendation | met |
| 7 monotone composition | met (unit) |
| 8 advisory vs catastrophic | met (unit) |
| 9 CLI flags both parsers | met |
| 10 synthetic corrupt fixture | met (page + block classes asserted separately) |
| 11 attach vec cap + report surface | met (cap + ExportSection totals wired) |
| 12 clean corpus behavior | met (aspose golden + dual-rate poly strip; telemetry pre-strip) |
| 13 numbers-only new lines | met (scan + unique-pst export_risk line) |
| 14 unique-pst-export docs | met |
| 15 audit SEC-06 | met |
| 16 perf timing | **partial** — fixture-scale recorded (~39 ms aspose release); multi-GB residual |
| 17 full workspace gate | targeted package gates green this fix round |
| 18 review.md / conductor Completed | orchestrator |
| 19–22 CRC_SUSPECT + tier2 + fidelity + count | met (attach stream consumed + clean-twin + pre-strip telemetry) |
| 23 Desk banner | met |

## Residual gaps
- **D-0077-parallel-attrib** (0079)
- **D-0077-tracing-layer**, **desk-subscriber**, **gui** drill-down, **repair-diff**
- **D-0077-systematic-poly** (P3) — dual-rate shipped; true poly fingerprint/allowlist residual; streaming under-merge until keep-set rebuild
- DoD-16 multi-GB timing residual (fixture-scale only)
- Exit codes unchanged (0078)
- Process DoD-18 (review.md + conductor Completed) left to orchestrator

## Verification run (r2 fix round)

```
cargo fmt --all --check          OK
cargo clippy --workspace --all-targets -- -D warnings   OK
cargo test -p pst-reader         OK
cargo test -p dedup-engine       OK
cargo test -p pst-writer         OK
cargo test -p pst-dedup-cli      OK (poly unit + sparse clean-twin + aspose 17/17 + crc_integrity_0077)
cargo test -p pst-dedup-gui      OK (22)
```

## Files touched (primary)

- `crates/pst-reader/src/integrity_telemetry.rs` (`with_crc_scope`; docs)
- `crates/pst-reader/src/messaging/{message,attachment}.rs` (block-only docs; attach scope + reader flag)
- `crates/pst-reader/src/lib.rs`
- `crates/pst-dedup-cli/src/scan.rs` (dual-rate poly; integrity buffer; pre-strip telemetry)
- `crates/pst-dedup-cli/src/pst_materializer.rs` (extract/props/attach-stream CRC_SUSPECT)
- `crates/pst-dedup-cli/src/attach_probe.rs` (reader.crc_suspect → CrcSuspect; probe consumers)
- `crates/pst-dedup-cli/src/unique_pst_cmd.rs` (attach event totals; export_risk human line)
- `crates/pst-dedup-cli/src/unique_export_report.rs` (scan_preflight reason)
- `crates/pst-dedup-cli/tests/crc_integrity_0077.rs` (sparse taint + clean twin + DoD-10)
- `crates/dedup-engine/src/keepset.rs` (DoD-20 units)
- `crates/pst-dedup-gui/src/worker.rs` (CRC_SUSPECT integrity)
- `docs/deferred.md` (D-0077-systematic-poly dual-rate / P3)
- `conductor/0077-CrcNoiseAndExportRisk/implementation-notes.md`

## Not done by design
- No ledger commit / no git push
- Conductor not flipped Completed
- No exit-code changes
- No low|elevated|high vocabulary

## Fix round Codex luna (review.codex.md FAIL) — 2026-07-28

Closed **P1/P2** findings without exit-code / vocabulary / fatal-CRC changes.

### P1-A — Final attach stream CRC taint (DoD-19)
- `AttachRead` carries `Arc<AtomicBool>` CRC flag via `from_reader_with_crc` / `crc_suspect()`
- After successful stream write, production writer records `ATTACH_STREAM_CRC` at **Info** severity (not fail; does not increment `attachments_failed`)
- `StreamCrc::default_severity` → Info
- `WriterAttachAdapter` opens concrete `AttachmentDataReader`, wraps with `CrcFlaggingAttachReader` that ORs `reader.crc_suspect()` into the shared flag
- `PstAttachStreamSource::open_attachment_data_reader` preserves the concrete type
- Test: `stream_crc_suspect_emits_fidelity_event_not_fail` (pst-writer)

### P1-B — Poly strip → Tier-2 auto-allow (rule 10)
- **Removed** identity strip of `CRC_SUSPECT` from candidates / rows / integrity.csv / degraded tallies
- Dual-rate poly gate still classifies sources; records path keys in `GroupingContext::poly_class_sources`
- `allow_crc_suspect_tier2_for(path_key)` = operator flag **or** poly-class source
- Keep-set / rebuild / scan insert eligibility honor per-source poly auto-allow
- `CRC_SUSPECT` remains on integrity (honesty); raw counters feed `export_risk`
- `ScanOutcome.poly_class_sources` merged into grouping in unique-pst / keep-set / unique-eml
- `docs/deferred.md` D-0077-systematic-poly updated: auto-enable Tier-2, taint remains

### P2-A — Per-source `distinct_bad_bids`
- TLS `source_bad_bids` set (cleared by `begin_source`, not drained on flush)
- `begin_source()` / `end_source_delta(before)` APIs; scan uses them for per-file fields
- Unit: `source_local_distinct_bad_bids` (source B reports 1, not cumulative 3)

### P2-B — Degraded count accounting
- Removed with identity strip (no longer decrements degraded when poly)

### P2-C — BID mismatch fixture (DoD-10)
- `flip_message_block_trailer_bid` flips trailer bytes 4..12 on first message data block
- Assert `block_bid_mismatches > 0` alongside page + block CRC

### P2-D — Deserialize scan JSON (DoD-4)
- `FileScanStats` / `ScanSummary` derive `Deserialize`
- `skips` / `integrity_csv` get `#[serde(default)]`
- Unit: `pre_0077_scan_json_deserializes_with_defaults`

### Verification (Codex luna fix round)

```
cargo fmt --all --check                                    OK
cargo clippy --workspace --all-targets -- -D warnings      OK
cargo test -p pst-reader                                   OK (source_local_distinct + existing)
cargo test -p dedup-engine                                 OK (157)
cargo test -p pst-writer                                   OK (stream_crc_suspect + suite)
cargo test -p pst-dedup-cli --lib                          OK (91; pre_0077 deserialize)
cargo test -p pst-dedup-cli --test crc_integrity_0077      OK (4; BID + sparse + aspose 17/17)
cargo test -p pst-dedup-gui                                OK (22)
```

### Residual P3 only
- DoD-16 multi-GB before/after overhead proof
- D-0077-parallel-attrib (demoted P2→P3; sequential correct), tracing-layer, desk-subscriber, gui drill-down, repair-diff
- D-0077-systematic-poly true poly fingerprint/allowlist residual (dual-rate clear shipped)
- DoD-18 review.md + conductor Completed (orchestrator)
- unique-pst human summary still omits per-counter CRC detail (export_risk line only) — P3 polish
- CRC remains warning-only / non-fatal

## Fix round Codex final-gate P1 (review.codex.final.md FAIL) — 2026-07-28

Closed two remaining **P1** correctness findings without exit-code / vocabulary / fatal-CRC changes.

### P1-1 — Honest poly: clear CRC_SUSPECT (not keep-taint + Tier-2 auto-allow)
- **Removed** `GroupingContext::poly_class_sources` and `allow_crc_suspect_tier2_for` poly special case
- Dual-rate (`page≥0.50` AND `block≥0.50`) **clears** false-positive `CRC_SUSPECT` from candidates, retained report rows, and buffered integrity.csv rows for that source (reclassify)
- Raw `page_crc_*` / `block_crc_*` / pre-clear `crc_suspect_messages` retained for reporting and `export_risk`
- Telemetry: `FileScanStats.poly_class_crc: bool` + `ScanSummary.poly_class_crc_sources`
- Sparse real corruption (rate << 0.5) still taints and blocks Tier-2
- Aspose golden: Degraded=false / DegradedReasons empty; winner NIDs/hashes match pre-0077
- `D-0077-systematic-poly` residual = true poly fingerprint/allowlist only

### P1-2 — Final attach stream CRC → export_risk
- After unique-pst write, count `ATTACH_STREAM_CRC` fidelity events (Info) across volumes
- `ExportRiskInputs.attach_stream_crc_events: u64` (serde default 0)
- Count > 0 elevates Ok → `re_export_recommended` with reason `attach_stream_crc_events=N>0`
- Does **not** increase `attachments_failed` / attach_fail_rate
- Unit: `export_risk_attach_stream_crc_events_recommend_reexport`

### Also
- `D-0077-parallel-attrib` demoted **P2 → P3** (0079 residual; sequential path correct)

### Verification (this round)

```
cargo fmt --all --check                                    OK
cargo clippy --workspace --all-targets -- -D warnings      OK
cargo test -p pst-reader                                   OK
cargo test -p dedup-engine                                 OK
cargo test -p pst-writer                                   OK (stream_crc_suspect)
cargo test -p pst-dedup-cli                                OK (92 lib + aspose golden + crc_integrity_0077 + export_risk unit)
cargo test -p pst-dedup-gui                                OK (22)
```

No conductor Completed, no push, no ledger commit (per task).
