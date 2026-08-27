# 0102 — Export Oracle Inputs Attest

> **Placeholder.** Expand before GO. Do not implement from this file as-is.
> Minted 2026-08-27 from PR **#89** Bugbot while planning **0100** (not stolen into 0100).

- **Track ID:** 0102-ExportOracleInputsAttest
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** `C:\dev\Dedupe\conductor\`
- **Status:** Proposed — placeholder
- **Depends on:** 0099 (Completed)
- **Series:** P (Unique-PST defensibility) — **after 0101** unless oracle CI is failing
- **Closes:** `D-0099-oracle-inputs-attest`

---

## 1. Objective

Make unique-pst `export_oracle` actually compare 0099 `export_risk.inputs` attest fields.

## 2. Context

`SUMMARY_ALLOWLIST_KEYS` includes `"inputs"`. `normalize_summary_for_oracle` → `strip_keys_recursive` deletes **every** object key named `inputs`, including `export_risk.inputs`. `compare_integrity_counters` then pointers `/export_risk/inputs/effective_block_crc_read_rate` (etc.) on the normalized tree — they are always missing. 0099 DoD required those pointers **not** on the allowlist.

Live (`main` @ `45c29de`): allowlist still has `"inputs"`; pointers still listed.

## 3. In scope (sketch)

- Stop stripping `export_risk.inputs` (narrow the allowlist or rename the strip so root `inputs` ≠ nested policy object).
- Keep product attest fields **out** of the volatile allowlist.
- Test: two summaries that differ only in `effective_block_crc_read_rate` must **mismatch**.

## 4. Out of scope

- 0100 recipient TC. 0101 depth. Fingerprint. Changing `export_risk` vocabulary.
