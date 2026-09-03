# 0129 — Account `also-eml` in `phase_timings`

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Additive serde field (`#[serde(default)]`). Include in `accounted_ms()`.
> **Never** fudge `unaccounted_ms` to 0 (0079 contract). 0 when `--also-eml` off.
> Not frontend. No BCC. Do not steal **0100–0104**. Do not speed also-eml (0130).

- **Track ID:** 0129-AlsoEmlPhaseTimings
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** unique-pst phase instrumentation. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-03).
- **Status:** Ready — not started
- **Depends on:** **0079** `PhaseTimings` · **0107** `--also-eml`
- **Spec authored:** 2026-09-03 (placeholder → Ready)
- **Series:** U (unique-export INC* HITL residuals)
>
> **Closes / absorbs:** `D-0129-also-eml-phase-timings` (the INC* ~393 s instrumentation gap).
> **HITL:** 2026-09-02 wall **1043 s**; `qc_ms≈407s`, `write_ms≈187s`, `unaccounted_ms≈393s` — also-eml ran after QC (`stage=also_eml`) and is **not** a phase field. Operator-local pack gitignored. CI uses fixtures, not INC*.
>
> **Harness fold-in (2026-09-03):** `opencode-review.md` + `agy-review.md`. Timer before/after the `write_eml_pack` match (cancel/Err included). DoD-4 = `unique_pst_also_eml` + `accounted_ms` unit — not empty `phase_timings` filter. See §2.9 / §8 / §9.

---

## 1. Objective

Time the also-eml co-export the same way QC is timed, so operators can see that ~6–7 minutes of an INC*-class job is EML write, not mystery `unaccounted_ms`.

This is **honest clocks**, not a speed rewrite.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

0079: non-zero `unaccounted_ms` means an instrumentation gap. 0107 writes also-eml after QC; no clock is recorded. Docs table in `unique-pst-export.md` (~167–177) lists scan…quarantine and **omits** both `qc_ms` (owned by **0130**) and `also_eml_ms` (this track).

### 2.2 Live APIs (plan-time 2026-09-03, HEAD `f8cb240`; re-verify at execute)

| Surface | Fact |
|---|---|
| Schema | **41**. N/A (no bump). |
| `PhaseTimings` | `scan/deep_attach/resolve/materialize/prepare/write/report/verify/qc/quarantine` + computed `unaccounted_ms` / `total_ms`. `#[serde(default)]`. **No** `also_eml_ms`. |
| `accounted_ms()` | Sums those phases; does **not** include also-eml. |
| `unique_pst_cmd.rs` | `phase_timings.qc_ms = t_qc.elapsed()…` then later `stage=also_eml` + `write_eml_pack_from_keep_set` with **no** `Instant`. |
| Docs | `unique-pst-export.md` phase table has no `also_eml_ms` (and no `qc_ms` — **0130**). |

### 2.3 Pins

Keep `PhaseTimings` `Copy`. Additive field only. Do not force `unaccounted_ms` to 0 even if also-eml explains most of the INC* gap.

### 2.4 Tools (plan-time)

`ai-brains preflight` inited; ledger 0 pending / 0 drift; `scan --impact` LOW (conductor docs). Federated `output/` budget — ignore INC* packs.

### 2.8 Last-PR Cursor comments

PRs **#146, #145, #144, #143**: inline **0**, reviews **0**, Bugbot usage-limit only. **Decline**.

### 2.9 Product locks

- Add `also_eml_ms: u64` with serde default 0. Include in `accounted_ms()`. Keep `PhaseTimings` `Copy`.
- Timer: `let t_also_eml = Instant::now();` **before** `match write_eml_pack_from_keep_set(...)`; assign `phase_timings.also_eml_ms = t_also_eml.elapsed()…` **unconditionally after the match** (covers `Ok`, `Ok`+cancelled, and `Err`). Flag-off → field stays 0. Cancel-before-block / `prepare_incomplete` never enter the timer — 0 is correct; no extra zeroing.
- Docs table row for `also_eml_ms` only (do not steal 0130’s `qc_ms` row). Place after `verify_ms` / near `quarantine_ms`. If 0129 and 0130 land in one release, coordinate so the table does not churn twice.
- Oracle allowlist already strips `phase_timings` (`export_oracle.rs`) — additive field must not break parent↔HEAD oracle equality.
- unique-eml standalone `UniqueEmlSummaryOut` is **not** this field.
- Unaccounted remains `total − Σ(phases)` never forced to 0.

---

## 3. In scope

`PhaseTimings` + orchestration timer + `docs/unique-pst-export.md` `also_eml_ms` row + unit/fixture. unique-eml standalone command is not this co-export clock.

## 4. Out of scope

Speeding also-eml or QC (**0130**). `--jobs` / `D-0079-operator-multigb`. Also-eml classify (**0109** closed). Frontend. BCC-default. Adding `qc_ms` to the docs table (**0130**).

## 5. Preconditions

0107 co-export shipped. 0079 `finalize()` contract stays.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Older summaries lack the field | `#[serde(default)]` |
| Forcing unaccounted to 0 | Forbidden |
| Timer misses cancel/fail path | Clock the whole `if let Some(eml_dir)` work that actually ran |
| Docs table also adding `qc_ms` here | Leave for 0130 |

## 7. Definition of Done

- [ ] **DoD-1:** `also_eml_ms` present; 0 when `--also-eml` omitted; included in `accounted_ms()`.
- [ ] **DoD-2:** `unique-pst-export.md` table row for `also_eml_ms` (0 when off). Do not add `qc_ms` here.
- [ ] **DoD-3:** `tests/unique_pst_also_eml.rs`: when also-eml ran, `phase_timings.also_eml_ms` is `Some(n)` with `n > 0`; when omitted, `0`. Unaccounted still computed, never fudged to 0.
- [ ] **DoD-4:** `cargo test -p pst-dedup-cli --test unique_pst_also_eml` plus a unit that `accounted_ms()` includes `also_eml_ms`. **Do not** use `cargo test … phase_timings` as the primary gate (0 tests today).
- [ ] **DoD-5:** `review.md`; registry Completed; CHANGELOG; ledger FEATURE.

## 8. Verification

```powershell
Set-Location C:\dev\Dedupe
cargo test -p pst-dedup-cli --test unique_pst_also_eml
cargo test -p pst-dedup-cli accounted
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

If the `accounted` filter is empty at execute, name the new unit test in `review.md` and still run `--test unique_pst_also_eml`.

## 9. Deferred

| ID | Disposition |
|---|---|
| **D-0129-also-eml-phase-timings** | **Absorb — this track.** |
| 0079 “unaccounted = instrumentation gap” | **Partial absorb** — also-eml was the INC* gap; other gaps may remain. |
| **D-0079-operator-multigb** | **Decline** (not `--jobs`). |
| Bugbot #143–#146 | **Decline.** |
| Empty `phase_timings` cargo filter as DoD evidence | **Decline** (folded: use `unique_pst_also_eml`). |

## 10. Unblocks

Operators can read also-eml cost next to QC. **0130** documents `qc_ms` and sample vs structure.
