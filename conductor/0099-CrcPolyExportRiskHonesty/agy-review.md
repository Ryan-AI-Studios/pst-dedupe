# Antigravity Adversarial Review — Track 0099: CRC / Poly Export-Risk Honesty

- **Track ID:** `0099-CrcPolyExportRiskHonesty`
- **Reviewer:** Antigravity (Adversarial Code Auditor & Systems Architect)
- **Date:** 2026-08-26
- **Review Scope:** Review only (no implementation) — line-level forensic audit of `unique_export_report.rs`, CRC risk thresholds, poly-class heuristic interaction, and fail-closed telemetry.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0099-CrcPolyExportRiskHonesty/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0099-CrcPolyExportRiskHonesty/plan.md)

---

## 1. Executive Summary & Problem Diagnosis

On operator archives created with non-standard CRC implementations (such as Aspose / Permute-class PSTs on `INC0102784.pst` and `INC0102784-2.pst`), Track 0077's dual-rate classifier correctly identified both sources as `poly_class_crc = true` (`page_rate ≥ 0.50 && block_rate ≥ 0.50`) and cleared `CRC_SUSPECT` from identity surfaces, allowing all 4,055 unique messages to be verified and exported.

However, `compute_export_risk` evaluated raw global counters (`block_crc_read_rate = 1.000 > 0.15` and `attach_stream_crc_events = 6014 > 0`), unconditionally declaring the export **`not_export_ready`**.

This created a severe contradiction:
- **Scan Preflight:** `Ok` (cleared by dual-rate poly classifier).
- **Physical Export:** 4,055 / 4,055 messages written and verified.
- **Post-Export Risk Report:** `NotExportReady` (claiming medium corruption).

Track 0099 resolves this defect by evaluating CRC risk against the **effective (non-poly) error rate**, allowing counsel to attest to valid poly-class PST exports without weakening detection of real, localized physical corruption.

---

## 2. Adversarial Code Audit & Subroutine Rigor

### Audit 0099-1: Mathematical Rigor of `effective_block_crc_read_rate`
- **Specification (§3.1):**
  ```rust
  let non_poly: Vec<&CrcSourceClass> = sources.iter().filter(|s| !s.poly_class_crc).collect();
  let crc_sum: u64 = non_poly.iter().map(|s| s.page_crc_mismatches.saturating_add(s.block_crc_mismatches)).sum();
  let reads: u64 = non_poly.iter().map(|s| s.page_reads.saturating_add(s.block_reads)).sum();
  let effective = if reads == 0 { 0.0 } else { (crc_sum as f64 / reads as f64).clamp(0.0, 1.0) };
  ```
- **Auditor Verification:**
  1. *All-Poly Job (INC0102784):* `non_poly` is empty -> `reads == 0` -> `effective == 0.0`. Threshold evaluation sees 0.0 (< 0.01) -> `Ok`.
  2. *All-Clean Job:* `non_poly == sources` -> `effective == raw ≈ 0.0` -> `Ok`.
  3. *Mixed Poly + Localized Corruption Job:* (e.g. Source A is poly with 10k mismatched blocks; Source B is standard PST with 100 corrupted blocks out of 500 reads = 20%):
     - `crc_sum = 100`, `reads = 500`.
     - `effective = 0.200`.
     - Because `0.200 > 0.15`, post-export evaluation correctly triggers `NotExportReady`!
- **Verdict:** PASS. Mathematical isolation prevents poly noise from masking or diluting localized corruption.

### Audit 0099-2: Fail-Closed Isolation of `discount_attach_stream_crc`
- **Rule (§3.1):**
  `discount_attach_stream_crc = !sources.is_empty() && !sources.iter().any(|s| (s.page_crc_mismatches + s.block_crc_mismatches > 0) && !s.poly_class_crc)`
- **Auditor Verification:**
  - In `pst-writer`, `ATTACH_STREAM_CRC` events are accumulated globally across the volume.
  - If a job contains a non-poly source with CRC noise (`crc_noisy && !poly_class`), we cannot determine whether an attachment CRC event originated from the poly source or the failing source.
  - The specification strictly sets `discount_attach_stream_crc = false` in any mixed-corrupt scenario.
- **Verdict:** PASS. Rigorously fail-closed.

### Audit 0099-3: Reason Code Mutex & Format Consistency
- **Audit Requirement:**
  - Raw `block_crc_read_rate=1.000>0.15` must NEVER be emitted when effective rate was 0.0.
  - When poly discount is applied, emit `poly_class_crc_discounted`.
  - If the effective rate crosses a threshold (mixed job), emit `effective_block_crc_read_rate={:.3}>{threshold}`.
- **Auditor Verification:**
  - Handled cleanly in `compute_export_risk_with_thresholds` by keying label and rate emission to `inputs.effective_block_crc_read_rate`.
- **Verdict:** PASS.

### Audit 0099-4: Monotonicity & Fail-Closed Preconditions
- **Invariant:** `export_risk.level = scan_recommendation.max(post)`.
  - If scan preflight was `NotExportReady` (e.g. unreadable header or failed file), `export_risk.level` remains `NotExportReady`.
  - If `sources` is empty (e.g. cancelled before scan or corrupted summary), `effective_block_crc_read_rate = None` and discount flags are `false` (fallback to raw).
- **Verdict:** PASS.

---

## 3. Recommended Spec & Plan Amendments

1. **Update `plan.md` §Phase 1 (`unique_export_report.rs`):**
   - Implement `CrcSourceClass`, `PolyCrcRiskAdjustment`, and `poly_crc_risk_adjustment`.
   - Extend `ExportRiskInputs` with `#[serde(default)]` fields:
     - `effective_block_crc_read_rate: Option<f64>`
     - `poly_class_crc_discounted: bool`
     - `discount_attach_stream_crc: bool`
     - `poly_class_crc_sources: u64`
   - Update `compute_export_risk_with_thresholds` to evaluate against `effective_block_crc_read_rate` and `discount_attach_stream_crc`.
2. **Update `plan.md` §Phase 2 (`unique_pst_cmd.rs`):**
   - In `unique_pst_cmd.rs:L3054`, map `outcome.summary.files` -> `Vec<CrcSourceClass>`, compute adjustment, and pass to `ExportRiskInputs`.
3. **Update Definition of Done (DoD-1 & DoD-2):**
   - Verify all 11 test cases in the plan unit matrix.
   - Assert that synthetic localized CRC tests retain `not_export_ready`.
   - Assert all-poly jobs produce `export_risk.level == Ok` with `poly_class_crc_discounted` in reasons.

---

## 4. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready for implementation; design is architecturally sound and fail-closed)**
- **Complexity / Risk:** Low (pure telemetry calculation and reporting logic; zero on-disk binary format changes).
- **Execution Estimate:** 0.5 – 1 day.
