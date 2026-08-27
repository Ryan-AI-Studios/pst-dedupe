# 0099 — CRC / Poly Export-Risk Honesty

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open the matrix during implementation.

- **Track ID:** 0099-CrcPolyExportRiskHonesty
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → unique-PST integrity / 0077
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0077 · 0078 · 0098 (all **Completed**)
- **Spec authored:** 2026-08-26
- **Series:** P (Unique-PST defensibility)
>
> **Review fold-in (2026-08-26):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.8. Phase 0 / §3 matrix stay **closed**. Mapper unit test, table-driven §3.4, and `export_oracle` attest pointers are now DoD. Per-event attach-CRC split stays declined (`D-0099-attach-crc-job-level`).
>
> **Promotes:** `D-0077-systematic-poly` export-risk honesty half (close on DoD).
> **Residual after this track:** `D-0077-poly-fingerprint` (true CRC polynomial / Permute allowlist vs dual-rate heuristic). Streaming unique under-merge until keep-set rebuild stays 0077 residual. **`D-0099-attach-crc-job-level`:** write-time `ATTACH_STREAM_CRC` is a job-level sum; discount keys off scan-time source class.

---

## 1. Objective

Make unique-pst `export_risk` **attest-able on poly/Permute stores**: dual-rate poly-class CRC must not force `not_export_ready` (or even `re_export_recommended`) when scan preflight is `ok` and the only “corruption” signal is the same non-standard CRC that 0077 already cleared from identity. Localized medium failure must still refuse handoff. Never repair source PSTs in-tool. Never invent a fourth `export_risk` value.

---

## 2. Context (read before starting)

### 2.1 Operator evidence (INC0102784, post-0098)

`output/inc0102784-post-0098/` (operator-local; not in git). Same inputs/order as 0097: `INC0102784-2.pst` then `INC0102784.pst`.

| Signal | Value |
|---|---|
| Written / verify found | **4055 / 4055** (0098 closed the count gap) |
| Scan preflight | `ok` |
| `poly_class_crc_sources` | **2** (both sources dual-rate) |
| `export_risk` | **`not_export_ready`** |
| Reasons | `attach_stream_crc_events=6014>0` · `block_crc_read_rate=1.000>0.15` |
| `crc_suspect_messages` | 4099 (pre-clear hit count; dual-rate already cleared candidate taint) |
| Exit | **64** `ATTACH_SOFT_FAIL` only (depth-limit → **0101**, not this track) |

Counsel cannot swear the unique PST: the report says the medium is failing, while scan already classified both sources as poly-class and proceeded with identity.

### 2.2 Why preflight is `ok` while export_risk is not

Scan preflight (`dedup-engine` `compute_preflight`) keys on **skip / CRC-skip / failed-file / attach-probe fail** rates. It does **not** read `block_crc_read_rate`.

0077 dual-rate (`page≥0.50` AND `block≥0.50`) **clears** `CRC_SUSPECT` from candidates so identity is not tainted. Raw page/block counters are **never zeroed**. Unique-pst `compute_export_risk` then treats those raw rates as medium failure:

| Gate | Default | Effect today |
|---|---|---|
| `catastrophic_block_crc_read_rate` | **0.15** | `not_export_ready` |
| `max_block_crc_read_rate` | **0.01** | `re_export_recommended` (only if post still `ok`) |
| `attach_stream_crc_events > 0` | any | `re_export_recommended` (only if post still `ok`; still **named** once catastrophic) |

INC* hits catastrophic first (`1.000 > 0.15`). The 6014 attach-stream events are the **same** warning-only CRC on attach bytes (`AttachmentFidelityKind::StreamCrc`, Info, not `attachments_failed`). On a poly store every attach stream “fails” CRC for the same reason pages/blocks do.

0077 recorded this split on purpose: “`export_risk` still sees raw `block_crc_read_rate`.” That is the hole 0099 closes. 0077 decision 5 still holds: **export never lowers scan preflight**. INC* is the opposite bug — export **raises** past an `ok` preflight using CRC scan already classified as poly.

### 2.3 Live code snapshot (verified 2026-08-26)

| Surface | State |
|---|---|
| Dual-rate classifier | `scan.rs` `is_poly_class_crc` — `page_rate ≥ 0.50 && block_rate ≥ 0.50`; `FileScanStats.poly_class_crc`; `ScanSummary.poly_class_crc_sources` |
| Identity | poly-class **clears** `CRC_SUSPECT`; high **block alone** keeps taint and blocks Tier-2 |
| `export_risk` | `unique_export_report.rs` `compute_export_risk_with_thresholds` — **raw** `block_crc_read_rate` + uncapped `attach_stream_crc_events`; no poly input |
| Wire-up | `unique_pst_cmd.rs` passes `outcome.summary.block_crc_read_rate` and summed `report.attach_stream_crc_events` |
| Attach CRC | `pst-writer` production: successful stream with `reader.crc_suspect()` → Info `ATTACH_STREAM_CRC`; uncapped counter (0077 P1-2) |
| Vocabulary | `PreflightRecommendation` `{ ok, re_export_recommended, not_export_ready }` — frozen (0077 D4 / runbook §4) |
| Composition | `level = scan.max(post)` — export cannot lower scan |
| Docs | `docs/unique-pst-export.md` CRC table: `block_crc_read_rate ≥ 0.15` ⇒ medium failing, **no poly exception**. Runbook repeats the 0.15 constant |
| `--jobs` | not shipped; `D-0077-parallel-attrib` says aggregate CRC if it ever is. Fail closed: no poly discount without per-source `files[]` |

### 2.4 Product locks (0077 restated; 0099 does not reopen)

1. **One risk vocabulary.** No `low|elevated|high`. No fourth `export_risk` string.
2. **Export never lowers scan preflight.** Poly discount applies only to **post-export CRC-derived** inputs.
3. **CRC stays warning-only.** 0099 does not skip, repair, or rewrite source bytes.
4. **Sources stay read-only.** ScanPST on a **copy** only (0081). No in-tool repair.
5. **Raw telemetry never zeroed.** `page_crc_*` / `block_crc_*` / `poly_class_crc` / raw `block_crc_read_rate` / raw `attach_stream_crc_events` remain in JSON.
6. **Dual-rate stays the classifier.** `page≥0.50` AND `block≥0.50` is a **fixed product constant** (not a CLI flag). True polynomial fingerprint is **out of scope** (`D-0077-poly-fingerprint`).
7. **Threshold constants stay.** Advisory 0.01 / catastrophic 0.15 / attach 0.05 / 0.50. 0099 changes **which rate those thresholds see**, not the numbers.
8. **Additive JSON.** New fields `#[serde(default)]`. Older summaries remain readable.
9. **No production `unwrap`/`expect`.** Synthetic tests in CI; INC* re-smoke is operator-local.

### 2.5 Phase 0 classification (closed)

| CRC class | Dual-rate? | What it is | Identity today | Export-risk today | Export-risk after 0099 |
|---|---|---|---|---|---|
| **PolyClass** | page≥0.50 **and** block≥0.50 | Non-standard CRC (aspose / Permute-class). Computed≠stored is **not** evidence of corrupt bytes | `CRC_SUSPECT` cleared | raw 1.0 → `not_export_ready` | CRC-derived post evaluation **does not elevate** |
| **LocalizedBlock** | high block, page **<** 0.50 | Real data-block corruption | taint kept; Tier-2 blocked | raw rate can be catastrophic | **unchanged** — thresholds see this source’s rate |
| **Clean** | both rates low | Standard MS-PST CRC | no taint | `ok` | `ok` |
| **Unreadable attach** | n/a | Stream **Fail** (`attachments_failed` / `attach_fail_rate`) | n/a | advisory / catastrophic on fail rate | **unchanged** — never discounted |
| **Hard export failure** | n/a | `failed_volume_index`, `partial+failed_volume` | n/a | `not_export_ready` | **unchanged** |

`distinct_bad_bids` large / `exact=false` is **not** an `export_risk` gate today. On poly-class sources it is expected (every block looks “bad”). Docs must stop telling operators that inexact bids ⇒ re-export **when `poly_class_crc` is true**. Do not add a new refuse from this metric in 0099.

### 2.8 Dual-AI review disposition (2026-08-26)

Reviews: `conductor/0099-CrcPolyExportRiskHonesty/opencode-review.md` (vs `main` @ `20f7aae`) and `agy-review.md`. Neither asked to reopen dual-rate, vocabulary, monotone `max`, or in-tool repair.

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | Diagnosis exact: preflight never reads `block_crc_read_rate`; export_risk uses raw rate + attach-stream events | opencode | **Agree** (already locked) | §2.2 |
| O2 | `ScanSummary.files[]` already carries every `CrcSourceClass` field; §3.3 map is supported | opencode | **Agree** | §3.3 |
| O3 | Exactly six `ExportRiskInputs` literals on `main` (4 tests + 2 call sites); GUI has none | opencode | **Agree** — closed edit set | plan Phase 1 |
| O4 | Phase 1 lib tests miss the `files[]` → `CrcSourceClass` mapper; a broken mapper reintroduces the lie | opencode | **Agree** — factor `crc_source_classes_from_files` + unit test | §3.3; DoD-3 |
| O5 | Table-drive spec §3.4 as a `#[test]` array | opencode | **Agree** as the preferred test shape | DoD-3; plan Phase 1 |
| O6 | `export_oracle` `compare_integrity_counters` will not see new attest fields unless the pointer list grows | opencode | **Agree as DoD** (not optional). Do **not** put them on `SUMMARY_ALLOWLIST_KEYS` (product, not volatile) | §3.3; DoD-3 |
| O7 | `poly_class_crc_discounted` can co-occur with `scan_recommendation=not_export_ready` | opencode | **Agree** — honest; document | §3.5 |
| O8 | Job-level `attach_stream_crc_events` vs scan-time `crc_noisy`: poly+clean could theoretically discount a clean source’s write-time attach CRC | opencode | **Agree to record, decline to split.** Per-event writer attribution stays out. Residual **D-0099-attach-crc-job-level** | §3.7; deferred.md |
| O9 | §8 should also run `cargo test -p dedup-engine` (monotone-max lives there) | opencode | **Agree** | §8 |
| O10 | Add `Default` for `ExportRiskInputs` so literals can `..Default::default()` | opencode | **Agree** as impl convenience, not a second policy | plan Phase 1 |
| O11 | Leave `--fail-on-export-risk` parse and 0078 exit contract untouched | opencode | **Agree** (already out of scope) | §4 |
| A1 | Effective rate uses non-poly sums; all-poly → `0.0`; mixed localized 0.20 still NER | agy | **Agree** — lock `saturating_add` | §3.1 |
| A2 | `discount_attach_stream_crc` only when no `(crc_noisy && !poly)` source | agy | **Agree** — lock the predicate | §3.1 |
| A3 | Empty `sources` → `effective=None`, flags false; monotone `max` | agy | **Agree** (already locked) | §3.1; §3.4 |
| A4 | Reason mutex: never emit raw `block_crc_read_rate=1.000>0.15` when effective was 0.0 | agy | **Agree** (already locked) | §3.5 |
| A5 | Plan Phase 1/2 field list + unique_pst_cmd wire-up at success-path `compute_export_risk` | agy | **Agree** — already in plan; keep | plan Phase 1–2 |
| A6 | DoD must keep localized synthetic CRC as `not_export_ready` | agy | **Agree** (already DoD-2) | §7 |

**Declined / not locked**

- Per-event / per-source attach-CRC counters in the writer (0099 stays job-level fail-closed).
- True CRC polynomial fingerprint (`D-0077-poly-fingerprint`).
- A fourth `export_risk` value, lowering scan preflight, zeroing raw CRC, changing 0.15 / 0.50.
- Allowlisting the new attest fields as oracle-volatile.

---

## 3. Design (locked)

### 3.1 Helper — `poly_crc_risk_adjustment`

New types in `crates/pst-dedup-cli/src/unique_export_report.rs` (same crate as `compute_export_risk`; do not push policy into `pst-reader`).

```rust
pub struct CrcSourceClass {
    pub poly_class_crc: bool,
    pub page_crc_mismatches: u64,
    pub block_crc_mismatches: u64,
    pub page_reads: u64,
    pub block_reads: u64,
}

pub struct PolyCrcRiskAdjustment {
    /// Non-poly CRC sum / non-poly reads. `None` if per-source stats missing (fail closed).
    pub effective_block_crc_read_rate: Option<f64>,
    /// True when ≥1 poly-class source was excluded from the rate used for thresholds.
    pub poly_class_crc_discounted: bool,
    /// True when no CRC-noisy non-poly source exists (attach CRC can only be poly noise).
    pub discount_attach_stream_crc: bool,
    pub poly_class_crc_sources: u64,
    pub non_poly_crc_noisy_sources: u64,
}

pub fn poly_crc_risk_adjustment(sources: &[CrcSourceClass]) -> PolyCrcRiskAdjustment;
```

**CRC-noisy:** `page_crc_mismatches.saturating_add(block_crc_mismatches) > 0`.

**Effective rate** (when `sources` is non-empty):

```
non_poly = sources where !poly_class_crc
crc_sum  = Σ saturating_add(page_crc, block_crc) over non_poly
reads    = Σ saturating_add(page_reads, block_reads) over non_poly
effective = 0.0 if reads == 0 else clamp(crc_sum as f64 / reads as f64, 0, 1)
```

All-poly job ⇒ `reads == 0` ⇒ `effective = 0.0` (no non-poly medium to judge). Mixed poly + localized ⇒ effective is **only** the localized source. Mixed poly + clean ⇒ effective ≈ 0 from the clean source.

**`discount_attach_stream_crc`:**

```
!sources.is_empty()
  && !sources.iter().any(|s| crc_noisy(s) && !s.poly_class_crc)
```

**`poly_class_crc_discounted`:** `sources` non-empty AND `poly_class_crc_sources ≥ 1` AND effective rate was computed (per-source path).

**Empty `sources`:** `effective = None`, both discount flags **false** (fail closed → caller uses raw).

### 3.2 `ExportRiskInputs` (additive)

Keep raw fields. Add (all `#[serde(default)]`):

| Field | Default | Role |
|---|---|---|
| `effective_block_crc_read_rate` | `None` | Rate **thresholds** use when `Some` |
| `poly_class_crc_discounted` | `false` | Attest + reason |
| `discount_attach_stream_crc` | `false` | Skip `attach_stream_crc_events>0` advisory |
| `poly_class_crc_sources` | `0` | Telemetry copy |

**Threshold keying:**

- `block_crc_read_rate` for advisory/catastrophic: use `effective_block_crc_read_rate` if `Some`, else raw (old callers / cancel path).
- `attach_stream_crc_events > 0` advisory: skip when `discount_attach_stream_crc`.
- `attach_fail_rate`, `failed_volume_index`, `partial+failed_volume`, `degraded_winner_rate`: **never** discounted.

When a CRC threshold is skipped because of poly discount, **do not** emit `block_crc_read_rate=1.000>0.15` from the raw rate. That reason is the current lie. Emit the closed reason `poly_class_crc_discounted` instead (and keep raw numbers on `inputs`).

If effective rate still crosses 0.15 (localized sibling in a mixed job), emit the **effective** crossing: `effective_block_crc_read_rate=0.200>0.15`.

### 3.3 Unique-pst wire-up

Factor a tested mapper (same crate as the helper; if a `scan` ↔ `unique_export_report` cycle appears, keep the mapper next to the unique-pst call site and `pub(crate)` it for tests):

```rust
pub fn crc_source_classes_from_files(files: &[crate::scan::FileScanStats]) -> Vec<CrcSourceClass>;
```

Map 1:1: `poly_class_crc`, `page_crc_mismatches`, `block_crc_mismatches`, `page_reads`, `block_reads`. Do **not** pre-average rates here — `poly_crc_risk_adjustment` owns the sums.

In `unique_pst_cmd.rs` (success path `compute_export_risk`, ~3054, and any other call that has a `ScanSummary`):

1. `let classes = crc_source_classes_from_files(&summary.files);`
2. `let adj = poly_crc_risk_adjustment(&classes);`
3. Pass raw rates **and** `adj` fields into `ExportRiskInputs`.

**`export_oracle` (DoD, not optional):** `compare_integrity_counters` today ends at `/export_risk/level` + `/scan/block_crc_rate` + `/scan/block_crc_read_rate` (`export_oracle.rs`). Extend the pointer list with:

- `/export_risk/inputs/effective_block_crc_read_rate`
- `/export_risk/inputs/poly_class_crc_discounted`
- `/export_risk/inputs/discount_attach_stream_crc`
- `/export_risk/inputs/poly_class_crc_sources`

Do **not** add those keys to `SUMMARY_ALLOWLIST_KEYS` — they are product attest fields, not volatile measurement. Leave `--fail-on-export-risk` parse and 0078 exit integers unchanged.

**Fail closed — do not discount (leave `effective = None`, flags false):**

- `files` empty (cancel / no scan files).
- Future `--jobs > 1` with aggregate CRC (`D-0077-parallel-attrib`). Comment at the call site: if per-source `poly_class_crc` is omitted, skip adjustment. `--jobs` is not shipped today.

Cancel-path constructor (~1125) stays zeros / no discount.

### 3.4 Locked export-risk matrix

Assume scan preflight `ok` and no failed volume. `post` then `level = max(scan, post)`.

| Job shape | Effective rate | Discount attach CRC? | `post` |
|---|---|---|---|
| All poly (INC*) | 0.0 | yes | **`ok`** + reason `poly_class_crc_discounted` |
| All clean | = raw ≈ 0 | n/a (flag false; no poly sources) | `ok` |
| Localized only, rate 0.20 | = raw 0.20 | no | **`not_export_ready`** (existing test) |
| Poly + clean | ≈ 0 from clean | yes | `ok` + `poly_class_crc_discounted` |
| Poly + localized 0.20 | 0.20 from localized | **no** (fail closed on attach CRC) | `not_export_ready` from effective; attach events still named |
| All poly + `attach_fail_rate` 0.06 | 0.0 | yes | `re_export_recommended` (fail rate, not CRC) |
| All poly + failed volume | 0.0 | yes | `not_export_ready` |
| Scan `not_export_ready` + all poly | 0.0 | yes | **`not_export_ready`** (monotone) |
| Scan `re_export_recommended` + all poly | 0.0 | yes | `re_export_recommended` |

Poly-class CRC **alone** must not raise post evaluation to either advisory or catastrophic. `--fail-on-export-risk` therefore does not fire on INC*-like CRC noise. Exit 64 from attach depth remains 0101.

### 3.5 Reasons vocabulary (closed, sorted, existing style)

| Reason | When |
|---|---|
| `poly_class_crc_discounted` | `poly_class_crc_discounted == true` |
| `effective_block_crc_read_rate={:.3}>{threshold}` | effective (or raw-if-None) crossed advisory or catastrophic |
| `attach_stream_crc_events={n}>0` | events > 0 **and** not `discount_attach_stream_crc` |
| existing `attach_fail_rate=…`, `failed_volume_index=…`, `scan_preflight=…`, `scan_recommendation=not_export_ready` | unchanged |

Do **not** emit raw `block_crc_read_rate=1.000>0.15` when the threshold used effective 0.0.

Reasons are a sorted set: **`poly_class_crc_discounted` may co-occur** with `scan_recommendation=not_export_ready` (or attach-fail / failed-volume reasons). Both are true; `level` is still `max(scan, post)`. The wizard banners on `level`. Docs (Phase 3) must say flags can co-occur when a non-CRC gate fired.

### 3.6 Docs delta (implementation Phase 3)

`docs/unique-pst-export.md` CRC table and `docs/unique-pst-ediscovery-runbook.md` integrity table:

- `block_crc_read_rate ≥ 0.15` means medium failure **unless** that rate is poly-class noise excluded via `effective_block_crc_read_rate`.
- `poly_class_crc` / `poly_class_crc_discounted`: computed≠stored is the store’s CRC, not a bad image. Raw counters remain for the affidavit.
- `distinct_bad_bids` large / `exact=false` on a poly-class source is expected; do not treat as widespread corruption.
- `ATTACH_STREAM_CRC` Info on a poly-class-only job is the same noise; it does not elevate `export_risk` after 0099.
- Vocabulary still frozen. ScanPST / never-repair paragraphs unchanged.

Desk wizard already banners on `export_risk.level`. No CRC drill-down UI (`D-0077-gui` stays residual). After 0099, INC*-like jobs stop painting `not_export_ready`.

### 3.7 Declined

| Idea | Why not |
|---|---|
| True CRC polynomial fingerprint / Permute allowlist | Residual `D-0077-poly-fingerprint`. Dual-rate already shipped; fingerprint overfits INC* and is a reader crypto track |
| Lowering scan preflight when poly | Violates 0077 decision 5 |
| New `export_risk` value (`poly_ok`, `crc_explained`, …) | Frozen three-word vocabulary |
| Zeroing raw CRC counters | 0077 lock 5; affidavit needs the raw 1.000 |
| Discounting `attach_fail_rate` | Fail severity is unread bytes, not CRC trailer mismatch |
| Per-event attach CRC split in the writer | Volume counter is uncapped but not per-source; job-level discount is fail-closed and enough for INC*. Residual **D-0099-attach-crc-job-level** |
| Changing 0.15 / 0.50 constants | Not the bug |
| BCC, recipient Strategy A, nested depth | **0100** / **0101** / 0082 |

---

## 4. Out of scope (do NOT do here)

- In-tool repair / ScanPST on evidence (0081: copy only).
- Recipient Strategy A (**0100**). Nested depth flag (**0101**).
- BCC write default (**0082** locked).
- `D-0093-attachment-tc-page`.
- Series O frontend (IDs **0105+**).
- `D-0077-gui` per-source CRC tables.
- `D-0077-parallel-attrib` / shipping `--jobs`.
- Polynomial CRC fingerprint (`D-0077-poly-fingerprint`).
- Changing CRC from warning-only to fatal.
- Mutating 0077 dual-rate thresholds or identity clear/keep rules.
- Changing `--fail-on-export-risk` parse or 0078 exit integers.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0077 dual-rate + `export_risk` vocabulary + uncapped `attach_stream_crc_events` — **Completed**.
- **P2:** 0098 verify counts match on INC* — **Completed** (operator-local). Handoff still blocked on this track’s `export_risk` lie.
- *Verified to date:* §2.1–2.3; `compute_export_risk` has no poly input; preflight ignores `block_crc_read_rate`.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Discounting real corruption as poly | Dual-rate requires **both** page and block ≥ 0.50. High block alone stays LocalizedBlock. Mixed jobs keep localized rate and do **not** discount attach CRC |
| Overfit to INC* Permute | Unit matrix in §7; second class is the existing 0077 synthetic localized CRC fixture (must still `not_export_ready`). No client bytes in git |
| Operators miss that CRC still fired | Raw rates stay on `inputs`; reason `poly_class_crc_discounted` always when applied |
| `--jobs` aggregate CRC later | Fail closed: no `files[]` poly flags ⇒ no discount; comment at call site |
| Existing export_risk tests break | Additive fields with `serde(default)`; update struct literals; keep catastrophic-without-poly test |
| Wizard still scary after level is `ok` | Banner follows `level`; no extra GUI work |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Policy in code:** `poly_crc_risk_adjustment` + `compute_export_risk` implement §3. All-poly INC*-like inputs (raw `block_crc_read_rate=1.0`, `attach_stream_crc_events=6014`, scan `ok`) produce `export_risk.level == ok` and reason `poly_class_crc_discounted`. Raw rates still present on `inputs`.
- [ ] **DoD-2 — Fail closed:** Localized-only `block_crc_read_rate=0.20` still `not_export_ready`. Catastrophic / advisory `attach_fail_rate` still elevate. Failed volume still `not_export_ready`. Scan `not_export_ready` still cannot be lowered. Mixed poly+localized uses effective localized rate and does not discount attach CRC.
- [ ] **DoD-3 — Tests:** Unit tests in `plan.md` Phase 1, including `crc_source_classes_from_files` (poly+localized, no pre-averaged rates) and a **table-driven** §3.4 matrix. Existing `export_risk_catastrophic_read_rate_without_failed_volume` and `export_risk_attach_stream_crc_events_recommend_reexport` still pass (no poly flags). Oracle pointer list includes the four attest fields. No client PSTs in git.
- [ ] **DoD-4 — Docs:** `unique-pst-export.md` + `unique-pst-ediscovery-runbook.md` state the poly exception **and** that `poly_class_crc_discounted` may co-occur with a non-CRC `not_export_ready` reason. `docs/deferred.md` closes the export-risk half of `D-0077-systematic-poly`, parks `D-0077-poly-fingerprint`, and records **`D-0099-attach-crc-job-level`**. CHANGELOG Unreleased line.
- [ ] **DoD-5 — Recorded:** `review.md`; registry **Completed**; ledger transaction committed (`FEATURE` or `BUGFIX` — implementation tx, not this planning tx). Optional operator INC* re-smoke in `review.md` (not CI).

---

## 8. Verification commands (reference)

```powershell
cargo test -p pst-dedup-cli --lib export_risk
cargo test -p pst-dedup-cli --lib poly_crc
cargo test -p pst-dedup-cli --lib crc_source
cargo test -p pst-dedup-cli
cargo test -p dedup-engine
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Optional operator smoke (not CI; never `git add` the PST):

```powershell
# after implementation, same order as post-0098
.\target\release\pst-dedup.exe unique-pst --policy first_seen --overwrite --json `
  C:\Users\RyanB\Desktop\Desktop\INC0102784-2.pst `
  C:\Users\RyanB\Desktop\Desktop\INC0102784.pst `
  --out C:\dev\Dedupe\output\inc0102784-post-0099\
# expect: export_risk.level ok (or not_export_ready only for non-CRC reasons);
# poly_class_crc_discounted in reasons; verify 4055/4055 still
```
