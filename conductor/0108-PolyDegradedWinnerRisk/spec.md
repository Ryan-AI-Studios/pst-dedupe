# 0108 — Poly-class CRC must not inflate `degraded_winner_rate`

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open 0099 block-rate discount, also-eml
> classify (0109), keep-set winner ranking, HNBITMAPHDR, BCC default, or frontend (0110+).

- **Track ID:** 0108-PolyDegradedWinnerRisk
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `docs/unique-pst-export.md` + this track. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-29); do **not** chase it at execute.
- **Cross-repo contract:** n/a
- **Status:** Ready — not started
- **Depends on:** 0099 (Completed — poly discount for `block_crc_read_rate` / `ATTACH_STREAM_CRC`) · D-0094 HITL 2026-08-29
- **Spec authored:** 2026-08-29 (placeholder → Ready)
- **Series:** S (Unique-export HITL residuals, post-0107)
>
> **Closes:** `D-0108-poly-degraded-winner-risk`.
> **HITL:** optional operator re-smoke of INC* (`output/inc0102784-post-0108/`, gitignored). **Not CI.** Do not commit client PSTs.
>
> **Last-PR fold-in (2026-08-29):** PRs **#105, #104, #103, #102**. Disposition in §2.8.
>
> **Review fold-in (2026-08-29):** `opencode-review.md` + `agy-review.md`. Disposition in §2.10 and `foldin-note.md`. Locks: §3.7 tests cover `AttachStreamCrc` / `CrcMismatch` / `unique==0`; degrade-reason emission **Ok-branch only** (match live raw); docs are **additive table rows**; mutate `inputs_from_sources` return (do not change helper signature); manual `Default` for new `ExportRiskInputs` fields; path tests use `\\?\` + compare-key case-differ.
>
> **Not frontend.** Series O is **0110+**. **0109** owns PR #104 Bugbot. No BCC-default track.

---

## 1. Objective

Make unique-pst `export_risk` key `degraded_winner_rate` the same way 0099 keyed `block_crc_read_rate`: **poly-class CRC taint must not be the reason a structurally clean INC* job looks 100% degraded.**

Today 0099 discounts raw block/attach CRC, but `compute_export_risk` still keys the **raw** `keep_set.stats.degraded_winners / unique` rate. HITL 2026-08-29: every winner is `integrity.degraded` because `CRC_SUSPECT` (4055/4055), so `degraded_winner_rate=1.000>0.02` → `re_export_recommended` even though scan preflight is `ok`, attach fails are 0, and verify is 4055/4055.

This track adds **`effective_degraded_winner_rate`**: exclude winners whose *only* degrade reasons are poly CRC (`CrcSuspect` and/or `AttachStreamCrc`) **and** whose source is `poly_class_crc`. Real body/attach degrade still counts. Raw `degraded_winner_rate` stays on `inputs`. Keep-set rows stay degraded (do **not** restrip `CRC_SUSPECT` — that would change fidelity ranking).

This advances unique-export **defensibility**: counsel sees poly noise as attest (`poly_class_crc_discounted`) plus any *real* degrade rate, not a fake 100% taint.

---

## 2. Context (read before starting)

### 2.1 Diagnosis (plan-time 2026-08-29; re-verify line numbers at execute)

HEAD `ba04519`. HITL artifacts `output/inc0102784-post-0107/` (gitignored).

| Surface | Live state |
|---|---|
| `unique_pst_cmd.rs` ~3303–3320 | `degraded_winner_rate = degraded_winners / unique`; passed raw into `ExportRiskInputs`. 0099 only fills `effective_block_crc_read_rate` / attach-CRC discount. |
| `unique_export_report.rs` ~632–638 | If `degraded_winner_rate > max_degraded_winner_rate` (0.02) → `re_export_recommended` + reason `degraded_winner_rate={:.3}>0.02`. **Never** reads an effective degrade rate. |
| `keepset.rs` ~2123–2125 | `degraded_winners++` when `item.integrity.degraded`. |
| `scan.rs` ~1248–1286 | Poly-class **clears** `CRC_SUSPECT` from scan candidates (`degraded_messages=0` on HITL). `crc_suspect_messages` stays the **pre-clear** count (4099). Comment: identity proceeds without taint. |
| unique-pst keep-set | **Re-taints.** HITL `keepset.json` winners: **4055/4055** `degraded` with `CRC_SUSPECT`. Reason sets: **3931** `{CRC_SUSPECT}` only; **124** `{BODY_UNAVAILABLE, CRC_SUSPECT}`. **Zero** `AttachStreamCrc` on winner rows **on this INC\* soak** (6034 events are the attach **ledger**, already 0099-discounted) — empirical, not a structural guarantee that winner-level `AttachStreamCrc` is unreachable. Unit tests must still construct `{AttachStreamCrc}` directly (§3.7). |
| 0099 spec §3.2 | `degraded_winner_rate`: **never** discounted. Test `export_risk_all_poly_inc_like_ok` (`unique_export_report.rs` ~2674) uses `inputs_from_sources` → `degraded_winner_rate` **defaults 0**, so CI never saw the INC* lie. |
| 0099 matrix “All poly → `ok`” | True only if degrade rate is also non-elevating. Real INC* after this track: see §3.4. |

**Do not** treat structural INC as open. TC / SLBLOCK / depth / also-eml / window-edge are shipped.

### 2.2 Why keep-set stays tainted (do not “fix” ranking here)

Scan poly-clear strips `CrcSuspect` from `RecoverableScanItem` used for identity. unique-pst `resolved.to_keep_set()` (~2094) still emits `CRC_SUSPECT` on every winner — materialize re-reads the poly store and the keep-set copies that taint. Restipping keep-set would change **fidelity ranking** (0077: `CRC_SUSPECT` is graded tier 3). **0108 does not restrip.** Residual: `D-0108-keepset-crc-retaint` (P3, §9).

### 2.3 Tools / recall

Ledger 0 pending. ai-brains: 0099 keys block rate on effective non-poly; HITL `degraded_winner_rate=1.0` after that. Semantic recall timed out; lexical used. `ledgerful scan --impact` on this tree is **LOW** (docs/skills; `conductor/` gitignored; soak `output/` blew the 5k file budget — do not commit it).

### 2.8 Last-PR Cursor comments (mandatory)

| PR | Surface | Disposition |
|---|---|---|
| **#104** | 3 Cursor Bugbot: also-eml `ok`/fidelity from exit 0; summary rewrite drops cancel 130; cancel zeros also-eml counts | **0109** already owns. Do not steal. |
| **#105, #103** | docs merge-SHA | none |
| **#102** | unique-eml nested MIME | none on export_risk / keep-set degrade |

No new placeholder. Next free ID remains **0117**.

### 2.9 Review fold-in (2026-08-29)

| Id | Disposition |
|---|---|
| opencode-M1 | **Agree — fold** — §3.7 tests for `{AttachStreamCrc}` poly/non-poly, `{CrcMismatch}` fail-closed, `unique==0` both branches |
| opencode-m1 | **Agree — fold** — degrade reasons **only** in `post == Ok` (match live raw). §3.6 row 5 is advisory-only (no catastrophic degrade threshold) |
| opencode-m2 | **Agree — fold** — Phase 3 / §3.8 / §9 **update** existing `D-0108-keepset-crc-retaint`; do not add a second row |
| opencode-m3 | **Agree — fold** — operator-doc edits are **additive table rows**; “never discount” lives in deferred/0099 spec, not the CRC tables |
| opencode-m4 | **Agree — fold** — mutate helper-returned `inputs` in `export_risk_all_poly_inc_like_ok`; do **not** change `inputs_from_sources` signature |
| opencode-m5 | **Agree — fold** — add fields to the **manual** `impl Default for ExportRiskInputs` (`None` / `0`) |
| opencode-O1 | **Agree — partial** — **no** `skip_serializing_if` on the new keys (`None` serializes as JSON `null`). Do not spend this track on pre-0108 absent-key vs null compare |
| opencode-O2 | **Already covered** — §2.1 now states ledger-vs-winner `AttachStreamCrc` is INC\* empirical |
| opencode-O3 | **Agree — fold** — path tests: byte-identical `\\?\` strings + one compare-key-equal case-differ. `path_compare_key` is **Windows lowercase only** (no `\\?\` / slash rewrite) |
| agy matrix / helper / oracle | **Already covered** — matches §3; scaled 39+2 → `{:.3}` **0.049** pinned in §3.7 |
| agy else-branch degrade reasons | **Decline** — would emit degrade reasons on catastrophic `post`; live raw never does (`:649-678`). 0108 matches existing Ok-branch |
| agy “slash separator normalize” | **Decline** — `path_compare_key` is lowercase-only (`keepset.rs` ~1020–1027). Exact **or** compare-key, same as scan poly-clear |
| agy tests in `unique_pst_depth.rs` | **Decline** — unit tests stay in `unique_export_report.rs` `mod tests` (0099 pattern). No extra integration crate |

### 2.10 Research currency

| Claim | Source | Plan-time |
|---|---|---|
| `ExportRiskInputs` / `compute_export_risk` | `crates/pst-dedup-cli/src/unique_export_report.rs` | additive serde; `max_degraded_winner_rate=0.02` |
| Keep-set winner integrity | `KeepEntry.integrity: RecoverableIntegrity` | `degraded_reasons: Vec<IntegrityReason>` |
| Poly per source | `FileScanStats.poly_class_crc` + `path` | dual-rate ≥0.50/≥0.50 |
| Path match | `dedup_engine::keepset::path_compare_key` | same helper scan poly-clear uses |
| Oracle pointers | `export_oracle.rs` `compare_integrity_counters` | ends at 0099 four `export_risk.inputs` keys; **not** on `SUMMARY_ALLOWLIST_KEYS` |
| MS-PST | N/A this track | — |
| Schema / jobs | N/A (`matter-core` schema v39 unused) | — |

Re-verify line numbers at execute. `unique_export_report_v1` / `keep_set_v1` ids **not** bumped (additive `serde(default)` like 0099).

---

## 3. In scope

### 3.1 Poly-only degrade reasons (closed set)

A winner is **poly-only-degraded** iff:

1. `integrity.degraded` is true, and
2. `degraded_reasons` is **non-empty**, and
3. **every** reason is `IntegrityReason::CrcSuspect` or `IntegrityReason::AttachStreamCrc`.

Anything else (`BodyUnavailable`, `BodyTruncated`, `AttachStreamOpenFailed`, `OrphanedNode`, `AttachCloudLink`, `AttachProbeTruncated`, empty-reasons-but-degraded, …) is **real degrade**. `CrcMismatch` is skip-not-keep — if it appears on a winner, **count it** (fail closed).

### 3.2 Per-source exclusion (0099 analog)

Exclude a poly-only-degraded winner from the **keyed** rate only when **all** hold:

- `poly_class_crc_discounted == true` (0099 adjustment already ran on non-empty `files[]`)
- winner `locus.source_path` maps to a `FileScanStats` with `poly_class_crc == true` via exact path **or** `path_compare_key` (same as `clear_poly_false_positive_crc_suspect` ~1864–1867). `path_compare_key` is **Windows lowercase only** — it does **not** strip `\\?\` or rewrite `/` vs `\`. HITL works because `FileScanStats.path` and `MessageLocus.source_path` are byte-identical `\\?\` strings today. Tests must use that shape plus a case-differ compare-key pair (§3.7).

Fail closed (do **not** exclude):

- `poly_class_crc_discounted == false` (localized / no poly sources / empty `files`)
- source path unmatched in `files[]`
- mixed job: poly-only `CrcSuspect` on a **non-poly** source (real sparse CRC)

Helper lives next to `poly_crc_risk_adjustment` in `unique_export_report.rs` (`pub(crate)` is enough if tests are in the same module; `pub` if unique-pst tests in another integration crate need it — prefer unit tests in `unique_export_report.rs` like 0099).

```rust
pub struct DegradedWinnerRiskAdjustment {
    pub effective_degraded_winner_rate: Option<f64>,
    pub degraded_winners_poly_only: u64,
}

pub fn poly_degraded_winner_adjustment(
    unique: u64,
    winners: &[dedup_engine::keepset::KeepEntry],
    files: &[crate::scan::FileScanStats],
    poly_class_crc_discounted: bool,
) -> DegradedWinnerRiskAdjustment;
```

- `unique == 0` → `effective = Some(0.0)` when discounted, else `None`.
- `!poly_class_crc_discounted` → `effective = None`, `degraded_winners_poly_only = 0`.
- When `Some`: `effective = real_degraded_count as f64 / unique as f64` (clamp 0..=1). `real_degraded_count` = degraded winners **not** excluded. Denominator is **`unique`**, not “degraded remaining” (same as today’s raw rate).

Do **not** change `keep_set.stats.degraded_winners`.

### 3.3 `ExportRiskInputs` (additive)

Keep raw `degraded_winner_rate`. Add (`#[serde(default)]` **and** the **manual** `impl Default for ExportRiskInputs` at ~483–500 — this struct does **not** `derive(Default)`):

| Field | Default | Role |
|---|---|---|
| `effective_degraded_winner_rate` | `None` | Rate **thresholds** use when `Some` |
| `degraded_winners_poly_only` | `0` | Telemetry: how many winners were excluded |

Do **not** add `skip_serializing_if` on these keys (`None` → JSON `null`). Cancel-path `..Default::default()` (~1223) then inherits fail-closed `None` / `0`.

**Threshold keying** (0099 analog):

- `degraded_winner_rate` advisory: use `effective_degraded_winner_rate` if `Some`, else raw.
- `attach_fail_rate`, `failed_volume_index`, `partial+failed_volume`: still **never** discounted.

**Reason emission — `post == Ok` branch only** (match live raw at ~617–638). The already-catastrophic `else` (~649–678) surfaces attach-fail / block-CRC / attach-CRC for operator detail but **never** degrade-rate reasons today; **do not** add them in 0108.

When `post` is still `Ok` and the keyed degrade rate is the **effective** one: **do not** emit `degraded_winner_rate=1.000>0.02`. Emit `effective_degraded_winner_rate={:.3}>0.02` **only if** that effective rate still crosses 0.02; else no degrade-rate reason. Keep emitting `poly_class_crc_discounted` as 0099 (after the if/else, ~680).

Raw `degraded_winner_rate` **always** remains on `inputs` (HITL 1.0 must still serialize). There is **no** catastrophic degrade threshold — degrade can only produce `re_export_recommended`.

### 3.4 Unique-pst wire-up

In `unique_pst_cmd.rs` success path after `crc_adj` (~3311–3328, re-verify):

1. Keep computing raw `degraded_winner_rate` from `keep_set.stats`.
2. `let deg_adj = poly_degraded_winner_adjustment(keep_set.stats.unique, &keep_set.winners, &outcome.summary.files, crc_adj.poly_class_crc_discounted);`
3. Pass `effective_degraded_winner_rate` + `degraded_winners_poly_only` on `ExportRiskInputs`.

Cancel-path constructor (~1212–1224) stays zeros / `effective = None` (fail closed). `--jobs` not shipped; same empty-`files` fail-closed comment as 0099 (`D-0077-parallel-attrib`).

`--fail-on-export-risk` parse and 0078 integers unchanged.

### 3.5 Oracle (DoD, not optional)

Extend `compare_integrity_counters` with:

- `/export_risk/inputs/effective_degraded_winner_rate`
- `/export_risk/inputs/degraded_winners_poly_only`

Do **not** add those keys to `SUMMARY_ALLOWLIST_KEYS` (0102 lesson: allowlist strip deletes product objects).

### 3.6 Locked matrix (scan `ok`, no failed volume)

| Job shape | Raw degrade rate | Effective | `post` |
|---|---|---|---|
| All poly, all winners `{CrcSuspect}` only | 1.0 | **0.0** | **`ok`** + `poly_class_crc_discounted` |
| All poly, HITL shape 3931 poly-only + 124 also `BodyUnavailable` | 1.0 | **124/4055 ≈ 0.031** | **`re_export_recommended`** + `effective_degraded_winner_rate=0.031>0.02` + `poly_class_crc_discounted` |
| All poly + `attach_fail_rate` 0.06 | 1.0 | 0.0 | `re_export_recommended` (fail rate, not CRC) |
| Localized only, all `CrcSuspect`, not poly | 1.0 | `None` → raw 1.0 | `re_export_recommended` (`degraded_winner_rate=1.000>0.02`) |
| Poly + localized source with `CrcSuspect` winners | mixed | those localized winners **count** | **advisory** from **effective** degrade if > 0.02 (`re_export_recommended` only — no catastrophic degrade gate) **plus** existing 0099 block-rate post (which *can* be catastrophic) |
| Scan `not_export_ready` + all poly CrcSuspect-only | 1.0 | 0.0 | **`not_export_ready`** (monotone) |

**Do not** raise `max_degraded_winner_rate` to make INC* `ok`. The 124 body-unavailable winners are real. 0108 success is **stopping the 1.000 lie**, not a clean-bill stamp.

### 3.7 Tests (CI; no client PST)

Prefer unit tests in `unique_export_report.rs` beside 0099:

| Test | Assert |
|---|---|
| `export_risk_all_poly_inc_like_ok` **updated** | **Mutate** the struct returned by `inputs_from_sources` (`degraded_winner_rate=1.0`, `effective_degraded_winner_rate=Some(0.0)`). Do **not** change the shared helper signature (8+ 0099 call sites). Still `ok`; no `degraded_winner_rate=1.000>` reason |
| `export_risk_poly_crc_suspect_only_ok` | mapper: 3 winners `{CrcSuspect}` on poly files → effective 0.0 → `ok` |
| `export_risk_poly_attach_stream_crc_only` | `{AttachStreamCrc}` only: **excluded** on poly source, **counted** on non-poly. Construct the reason directly (HITL winners have none) |
| `export_risk_poly_crc_mismatch_fail_closed` | `{CrcMismatch}` on a poly-source winner is **counted** (not in the exclusion set) |
| `poly_degraded_unique_zero` | `unique==0 && discounted` → `Some(0.0)`; `unique==0 && !discounted` → `None` |
| `export_risk_poly_plus_body_unavailable_advisory` | scaled HITL 39 `{CrcSuspect}` + 2 `{BodyUnavailable,CrcSuspect}` (`unique=41`) → effective `2/41` formats as **`effective_degraded_winner_rate=0.049>0.02`**; raw 1.0 on inputs |
| `export_risk_localized_crc_suspect_still_raw` | `poly_class_crc_discounted=false`, raw 1.0, effective `None` → `degraded_winner_rate=1.000>0.02` |
| `export_risk_mixed_poly_plus_localized_crc_counts` | poly file CrcSuspect-only excluded; non-poly file CrcSuspect **counted** |
| `poly_degraded_unmapped_source_fail_closed` | unmatched `source_path` → winner counted |
| `poly_degraded_path_match` | byte-identical `\\?\C:\…` exact match; plus one compare-key-equal **case-differ** pair (`C:\Foo.pst` vs `c:\foo.pst` on Windows) |
| oracle pointer | `effective_degraded_winner_rate` mismatch detected; allowlist does not strip the key |

Do **not** import INC* `keepset.json` into git. Keep tests in `unique_export_report.rs` `mod tests` (construction-style like `poly_class()` ~2539).

### 3.8 Docs

**Additive rows** in the existing operator tables (there is **no** “never discount `degraded_winner_rate`” sentence in those files today — that phrasing lives in `docs/deferred.md` and the 0099 spec):

- `docs/unique-pst-export.md` CRC table (~294–309): poly-class keep-set `CRC_SUSPECT` does not, **by itself**, elevate `export_risk` when `effective_degraded_winner_rate` is used; raw `degraded_winner_rate` stays on `inputs`; body/attach still keys the effective rate (`max_degraded_winner_rate=0.02` unchanged).
- `docs/unique-pst-ediscovery-runbook.md` integrity table (~185–198): expand the “Never discounted: `attach_fail_rate`” family with the effective-degrade row. INC* after 0108 may stay `re_export_recommended` **if** non-poly degrade (e.g. `BODY_UNAVAILABLE`) exceeds 0.02 — disclose that, do not raise the cap.

Amend the **deferred / 0099** “never discount `degraded_winner_rate`” wording to “never discount **non-poly** degrade.” CHANGELOG Unreleased. Close `D-0108-poly-degraded-winner-risk`. **Update** (do not duplicate) `D-0108-keepset-crc-retaint`.

---

## 4. Out of scope (do NOT do here)

- Restipping `CRC_SUSPECT` from keep-set / scan-identity ranking (`D-0108-keepset-crc-retaint`).
- Raising `max_degraded_winner_rate`.
- True polynomial fingerprint (`D-0077-poly-fingerprint`).
- Per-event writer attach-CRC attribution (`D-0099-attach-crc-job-level`).
- Also-eml classify/cancel (0109). Frontend (0110+). HNBITMAPHDR. BCC default.
- In-tool ScanPST / CRC repair. Mutating source PSTs. Committing `output/` or INC* JSON.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0099 shipped (`poly_crc_risk_adjustment`, `ExportRiskInputs` poly fields, oracle 0099 pointers).
- *Verified to date:* HITL keep-set reason histogram (gitignored); live `compute_export_risk` keys raw degrade rate; scan poly-clear vs keep-set re-taint.
- Re-verify `IntegrityReason` set, `path_compare_key`, and unique-pst call site at execute.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Discounting real body/attach degrade | Exclusion only for the closed poly-only set **and** poly-class source |
| Mixed poly + localized CRC | Non-poly sources’ `CrcSuspect` winners still count |
| Changing the 4055 winner set | Do not restrip keep-set |
| Oracle / 0102 allowlist trap | New keys on pointer list only, never `SUMMARY_ALLOWLIST_KEYS` |
| Promising INC* `export_risk=ok` | §3.6 HITL effective ≈ 0.031 → still advisory; that’s success |
| `unwrap`/`expect` | Forbidden in production |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Policy in code:** `poly_degraded_winner_adjustment` + `compute_export_risk` implement §3. All-poly + CrcSuspect-only winners + scan `ok` → `export_risk.level == ok` and `poly_class_crc_discounted`; raw `degraded_winner_rate` still 1.0 on `inputs`; **no** `degraded_winner_rate=1.000>0.02` reason. CI includes `{AttachStreamCrc}` poly/non-poly, `{CrcMismatch}` fail-closed, and `unique==0` both branches (§3.7).
- [ ] **DoD-2 — Real degrade still elevates:** BodyUnavailable (or attach-open) winners on a poly job key **effective** rate; if > 0.02 → `re_export_recommended` with `effective_degraded_winner_rate=…` (not the raw 1.000 reason). Localized (non-poly) CrcSuspect still uses **raw** rate. Degrade-reason strings emit only when `post == Ok`.
- [ ] **DoD-3 — Keep-set / Tier-2 unchanged:** `keep_set.stats.degraded_winners` still counts CRC-tainted winners. `assess_tier2_eligibility` still refuses `CrcSuspect` without `--allow-crc-suspect-tier2`. No keep-set schema bump.
- [ ] **DoD-4 — Oracle:** pointers in §3.5 compare; allowlist does not strip them.
- [ ] **DoD-5 — Docs:** additive CRC/integrity table rows (§3.8); deferred **closes** `D-0108-poly-degraded-winner-risk` and **updates** existing `D-0108-keepset-crc-retaint` (no second row); CHANGELOG Unreleased.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; ledger commit (`FEATURE` or `BUGFIX`).
- [ ] **DoD-7 — HITL (optional, not CI):** INC* unique-pst; expect **not** `degraded_winner_rate=1.000>0.02`; expect `effective_degraded_winner_rate` ≈ 0.031 and `degraded_winners_poly_only` ≈ 3931; level may remain `re_export_recommended`. Artifacts under `output/` gitignored.

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p pst-dedup-cli --lib export_risk
cargo test -p pst-dedup-cli --lib poly_degraded
# plus existing 0099 names still green:
cargo test -p pst-dedup-cli --lib export_risk_all_poly_inc_like_ok
```

Optional operator (never CI, never commit):

```powershell
# after implement; same inputs as D-0094 HITL
.\target\release\pst-dedup.exe unique-pst `
  $inc1 $inc2 --source-rank INC0102784.pst --source-rank INC0102784-2.pst `
  --max-embedded-depth 8 --out output\inc0102784-post-0108\unique.pst `
  --report-dir output\inc0102784-post-0108\report --json --overwrite
```

---

## 9. Deferred (absorb / decline)

| Row | Disposition |
|---|---|
| `D-0108-poly-degraded-winner-risk` | **Absorb — this track** |
| `D-0108-keepset-crc-retaint` | **Already in `docs/deferred.md`** — **update** on complete (do not add a duplicate). Scan poly-clears `CRC_SUSPECT`; unique-pst keep-set re-taints. 0108 does not restrip. |
| `D-0109-also-eml-classify` | Decline — **0109** |
| `D-0077-poly-fingerprint` | Decline — later reader |
| `D-0077-gui` | Decline — banner already follows `export_risk.level` |
| `D-0099-attach-crc-job-level` | Decline — 0099 residual |
| `D-0077-parallel-attrib` | Decline — fail closed if `--jobs` ever drops per-source files |
| `D-0100-hn-bitmap-hdr` | Decline — fail-closed until corpus hits |
| `D-0079-reader-buffer` / `stream-prepare` | Decline — perf |
| `D-0067-embedded-depth` | Decline — matter children; unique-eml MIME shipped 0106 |
| 0082 BCC default | Decline |
| also-eml `unaccounted_ms` / fidelity events truncated | Decline — P3; CSV is source of truth |
| Frontend 0110–0116 | Decline |
| Raising unique-pst caps | Decline |

No med/high left in deferred for this surface: `D-0108` is P1 and owned here.
