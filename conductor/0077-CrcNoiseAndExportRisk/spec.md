# 0077 — CRC Noise Control & Export Risk Score

- **Track ID:** 0077-CrcNoiseAndExportRisk
- **Status:** Ready
- **Series:** L

## 1. Objective

Stop multi-hundred-MB stderr floods of repeated page CRC warnings; emit **aggregated integrity counters** and an **export risk score** so operators know when to re-export sources before unique-pst.

## 2. Context

- INC unique-pst stderr ~**246 MB**, ~**108k** `Page CRC mismatch` lines, ~1.6M other WARNs.
- Scan preflight showed skip_rate 0 — export path hits block CRC heavily when reading attach streams.
- Microsoft guidance: use ScanPST for structural repair; large/corrupt PSTs common on eDiscovery exports.
- Logging best practice: rate-limit identical keys; promote summary metrics.

## 3. In scope

1. Rate-limit CRC/page warnings (first N per bid/code, then summary every S seconds).
2. Counters on scan/export JSON: `page_crc_mismatches`, `unique_bad_bids`, `attach_stream_errors`.
3. `export_risk` enum: `low | elevated | high` from thresholds (CRC rate, attach fail rate, degraded winners).
4. Operator doc: when to ScanPST / re-export from Purview vs proceed with partial fidelity.
5. Default log level remains usable (`RUST_LOG=error` quiet; info shows summaries not floods).

## 4. Out of scope

- Repairing PST pages in-place.
- Changing CRC validation semantics (still detect; just report better).

## 5. DoD

- [ ] INC-scale re-run produces stderr << 10 MB at info (or documented cap)
- [ ] summary JSON includes counters + risk
- [ ] Tests for rate-limit + risk thresholds
- [ ] docs/operator note
