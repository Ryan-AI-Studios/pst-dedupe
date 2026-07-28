# Track Completion Audit — 0077-CrcNoiseAndExportRisk

## Verdict: PASS WITH DEFERRED P3

## Scope Reviewed

- Branch: `feat/0077-crc-noise-export-risk` (from `main@79b5cdf`)
- Spec/plan DoD-1..23; internal subagent r1–r3; Codex luna high rounds (FAIL → fix → final2 residual fixes)
- Crates: `pst-reader` (integrity_telemetry), `dedup-engine` (CRC_SUSPECT, fidelity), `pst-dedup-cli` (scan/export_risk/CLI), `pst-writer` (attach event cap + stream CRC counter), `pst-dedup-gui` (risk banner)
- Docs: `unique-pst-export.md`, `audit.md` SEC-06, `docs/deferred.md`

## Reviewers / Rounds

| Round | Result |
|---|---|
| Internal r1 | FAIL (P1 systematic strip, attach scope, tests) |
| Fix + r1b | P2 residuals |
| Internal r2 | FAIL (P2 strip/accounting/stream consumers) |
| Fix dual-rate + attach consumers | — |
| Internal r3 | PASS WITH DEFERRED P3 |
| Codex luna r1 | FAIL (stream CRC erase, poly strip rule 10, distinct BIDs, …) |
| Codex fix + final | FAIL (poly auto-allow, risk gap) |
| Poly reclassify + attach_stream_crc → export_risk | — |
| Codex final2 | FAIL (capped Vec count; basename report match) |
| Uncapped StreamCrc counter + buffer/flush by pst_index | fixed |
| Orchestrator gates | `fmt` / `clippy -D warnings` / `cargo test --workspace` **exit 0** |

## Requirement and DoD Matrix (summary)

| DoD | Status |
|---|---|
| 1–5 telemetry, rates, crc_skip_rate pin | Met |
| 6–8 export_risk vocabulary / monotone / tiers | Met |
| 9 CLI flags both parsers | Met |
| 10 synthetic page/block/BID fixture | Met |
| 11 attach event cap + total/truncated | Met |
| 12 clean corpus / aspose golden | Met (poly reclassifies false-positive CRC_SUSPECT) |
| 13 numbers-only new lines | Met (P3: unique-pst human CRC line polish residual) |
| 14–15 docs + SEC-06 | Met |
| 16 perf | Partial fixture-scale (~39 ms aspose); multi-GB residual P3 |
| 17 gates | Met (fmt/clippy/test workspace green) |
| 18 this review + conductor Completed | Met at finalize |
| 19–22 CRC_SUSPECT, Tier-2, fidelity, counts | Met |
| 23 Desk banner | Met |

## Key design (post-Codex)

1. **Data-path telemetry** — not a tracing Layer; counters always exact; emission first-N + interval.
2. **CRC_SUSPECT** — block CRC/BID message-scope taint; Tier-2 ineligible by default; Tier 1 untouched; `--allow-crc-suspect-tier2`.
3. **Poly dual-rate** (`page≥0.5` AND `block≥0.5`) — reclassify as false-positive: **clear** CRC_SUSPECT for identity; keep raw CRC counters for rates/export_risk; `poly_class_crc` flag. No Tier-2 auto-allow on still-suspect items.
4. **export_risk** — reuses `PreflightRecommendation`; advisory vs catastrophic; `attach_stream_crc_events` uncapped counter feeds risk.
5. **No exit-code change** (0078).

## Deferred (P3 only)

See `docs/deferred.md` D-0077-*:

- systematic-poly fingerprint residual
- parallel-attrib (0079)
- tracing-layer, desk-subscriber, gui drill-down, repair-diff
- DoD-16 multi-GB before/after

Closed by this track: **D-0074-crc-fixture**, **D-0073-vec-events**.

## Verification Evidence

```
cargo fmt --all --check                         OK
cargo clippy --workspace --all-targets -- -D warnings   OK
cargo test --workspace                          OK
```

Notable tests: `crc_integrity_0077` (4), aspose keep-set golden, `stream_crc_suspect_emits_fidelity_event_not_fail`, export_risk units, GUI banner mapping.

## Completion Decision

Engineering DoD met with deferred P3 only. Ready for PR / CI / squash merge.
