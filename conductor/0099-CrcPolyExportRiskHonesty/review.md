# Track Completion Review — 0099-CrcPolyExportRiskHonesty

## Verdict: PASS (internal gates) — cross-model round pending in this file if findings land

## Scope

Make unique-pst `export_risk` attest-able on poly/Permute stores: dual-rate poly-class CRC must not force `not_export_ready` / `re_export_recommended` when scan preflight is `ok` and the only signal is the same non-standard CRC 0077 already cleared from identity. Localized medium failure still refuses. Never repair source PSTs. Never invent a fourth `export_risk` value.

Closes **D-0077-systematic-poly** (export-risk honesty half). Residual **D-0077-poly-fingerprint**, **D-0099-attach-crc-job-level**.

Branch: `0099-crc-poly-export-risk-honesty`. Ledger: `a60b3c92-10e6-46f8-b8c9-6afeb35b078f` (`BUGFIX`, `crates/pst-dedup-cli`).

## Reviewers / rounds

| Round | Reviewer | Result |
|---|---|---|
| Spec | Dual-AI Ready (`opencode-review.md` + `agy-review.md`) | Folded into spec §2.8 before implementation |
| Internal | Implementer gates | `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test -p pst-dedup-cli --lib` (233); `cargo test -p dedup-engine` (239); `cargo test -p pst-dedup-cli --test unique_pst` (37); `cargo check -p pst-dedup-gui`. INC* re-smoke not run (operator-local). |

## DoD matrix

| DoD | Status | Evidence |
|---|---|---|
| DoD-1 | Met | `export_risk_all_poly_inc_like_ok`: raw `block_crc_read_rate=1.0`, `attach_stream_crc_events=6014`, scan `ok` → level `ok` + reason `poly_class_crc_discounted`; raw rates remain on `inputs`. |
| DoD-2 | Met | Localized 0.20 still NER (`export_risk_catastrophic_read_rate_without_failed_volume` without poly flags; mixed poly+localized table row). Attach fail 0.06 still advisory. Failed volume still NER. Scan NER cannot be lowered. Mixed job does not discount attach CRC. |
| DoD-3 | Met | Helper tests + `crc_source_classes_from_files_maps_raw_counters` + table-driven §3.4 (`export_risk_matrix_table_driven`). Oracle pointers added; not on `SUMMARY_ALLOWLIST_KEYS`. |
| DoD-4 | Met | Export docs + runbook poly exception; `poly_class_crc_discounted` co-occur documented. `D-0077-systematic-poly` closed; fingerprint parked; `D-0099-attach-crc-job-level` recorded. CHANGELOG Unreleased. |
| DoD-5 | Met | This `review.md`; registry **Completed**; ledger `BUGFIX` on `crates/pst-dedup-cli` (`unique_export_report` + `unique_pst_cmd`). Optional INC* re-smoke not run (operator-local). |

## Key locks honored

- One risk vocabulary (`ok` / `re_export_recommended` / `not_export_ready`)
- Export never lowers scan preflight (`max`)
- Raw CRC telemetry never zeroed
- Dual-rate 0.50/0.50 unchanged; 0.15 / 0.01 thresholds unchanged (keyed rate changed)
- `--fail-on-export-risk` parse and 0078 exit integers untouched
- `--jobs` not shipped; call-site comment fail-closed if per-source CRC omitted
- No production `unwrap`/`expect`
- No GUI CRC drill-down (`D-0077-gui` stays)

## Implementation note (spec §3.1 vs §3.4)

`discount_attach_stream_crc` requires `poly_class_crc_sources >= 1` in addition to “no CRC-noisy non-poly source.” Literal §3.1 `!any(crc_noisy && !poly)` would set the flag true on an all-clean job. §3.4 says all-clean flag **false**. Combined predicate matches the English (“attach CRC can only be poly noise”) and keeps write-time `ATTACH_STREAM_CRC` on a clean store elevating `export_risk`.

## Deferred

- **D-0077-poly-fingerprint** — true CRC polynomial / Permute allowlist
- **D-0099-attach-crc-job-level** — job-level attach CRC vs scan-time source class
- Operator INC* re-smoke → `output/inc0102784-post-0099/` (expect `export_risk.level=ok` + `poly_class_crc_discounted`; verify 4055/4055). Not CI.

## Operator note

Post-0098 INC0102784 unique-pst was 4055/4055 with scan preflight `ok` and `export_risk=not_export_ready` from raw `block_crc_read_rate=1.000` plus 6014 `ATTACH_STREAM_CRC`. After 0099 that CRC class must not refuse handoff; remaining exit 64 `ATTACH_SOFT_FAIL` (depth) is **0101**.
