# 0130 — QC-sample cost (default stays `sample`)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> **Default `--qc-level sample` stays.** Do not switch deliverable default to `structure` or `off`.
> Do not skip source-differential to go faster. Outlook COM declined 0080. No `--jobs`.
> Not frontend. No BCC. Do not steal **0100–0104**.

- **Track ID:** 0130-QcSampleCost
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** unique-pst QC honesty. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-03).
- **Status:** Ready — not started
- **Depends on:** **0080** QC (`--qc-level sample` default, `--qc-sample-max` 64)
- **Spec authored:** 2026-09-03 (placeholder → Ready)
- **Series:** U (unique-export INC* HITL residuals)
>
> **Closes / absorbs:** `D-0130-qc-sample-cost`. Keeps 0080 default **sample**.
> **HITL:** 2026-09-02 `qc_ms≈407s` (~39% of 1043 s) for 64 msgs / 312 attaches, defect 0, known_gap 51 (BCC-by-design). Not a correctness bug. Operator-local pack gitignored.
>
> **Harness fold-in (2026-09-03):** `opencode-review.md` + `agy-review.md`. `qc_ms=` on **both** `qc ok` and `qc hard findings`; stderr-format test hook; ledger FEATURE; stderr-first. See §2.9 / §7 / §9.

---

## 1. Objective

Make QC wall-time an operator choice they can **see**, not a surprise ~7-minute tax. Keep sample as the counsel-handoff default.

This track is **cost honesty**. It does **not** plan a sample-speed rewrite.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

0080 source-differential sample is the honesty gate. INC* 2026-09-02: QC is the largest **billed** phase once 0129 clocks also-eml. Operators running throughput passes need `--qc-level structure` documented; counsel unique stays `sample`.

### 2.2 Live APIs (plan-time 2026-09-03, HEAD `f8cb240`; re-verify at execute)

| Surface | Fact |
|---|---|
| Schema | **41**. N/A. |
| clap | `--qc-level` default **`sample`**. `--qc-sample-max` default 64. |
| `PhaseTimings.qc_ms` | Already filled after QC (`unique_pst_cmd.rs` ~3008). |
| stderr | `qc ok: level={} messages_compared={} known_gap={}` — **no** `qc_ms`. |
| Docs | `unique-pst-export.md` phase table **omits `qc_ms`**. Runbook has no sample-vs-structure timing table. |

### 2.3 Pins

Default sample stays. Source-differential stays. No Outlook COM. No `--jobs`.

**Sample-speedup: declined this track.** Placeholder allowed a measurement-gated cheaper sample. Ready plan: **docs + stderr `qc_ms` + docs table row only**. A later track may mint speedup only after execute proves a no-contract cache; do not invent one here.

### 2.4 Tools (plan-time)

`ai-brains preflight` inited; ledger 0 pending / 0 drift; `scan --impact` LOW (conductor docs). Federated `output/` budget — ignore INC* packs.

### 2.8 Last-PR Cursor comments

PRs **#146, #145, #144, #143**: inline **0**, reviews **0**, Bugbot usage-limit only. **Decline**.

### 2.9 Product locks

- Runbook timing table: `sample` (handoff) vs `structure` (throughput) vs `off` (not a substitute for sample) vs `full`. Do **not** claim `qc-pst` defaults differently (`main.rs` also defaults sample / 64).
- stderr token `qc_ms={}` (not `elapsed_ms=` / prose) on **both** lines:
  - `qc ok: level={} qc_ms={} messages_compared={} known_gap={}`
  - `qc hard findings: qc_ms={} defect={} unexplained_loss={}`
- `unique-pst-export.md`: add `qc_ms` row (0129 owns `also_eml_ms`).
- clap `default_value = "sample"` still. Tests: `unique_pst_qc_0080` plus a stderr capture asserting `qc_ms=` (existing `on_log` pattern in `unique_pst_cmd.rs` tests). `unique_pst_qc_0080` does **not** currently assert the `qc ok:` format.

---

## 3. In scope

Runbook + stderr `qc_ms` + export-docs `qc_ms` row. Optional one-line clap help if execute finds it silent on cost. No algorithm change unless a **bug** (wrong clock) is found.

## 4. Out of scope

Changing default to `structure`/`off`. Skipping source compare. Outlook COM. `--jobs`. 0129 also-eml clock. Sample-speed rewrite. Frontend. BCC-default.

## 5. Preconditions

0080 default sample shipped. `qc_ms` already recorded.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Speeding sample by skipping source compare | Forbidden — declined this track |
| Operators skip QC on counsel unique | Docs: structure ≠ sample |
| Docs-only looks unfinished | stderr `qc_ms` is the code check |

## 7. Definition of Done

- [ ] **DoD-1:** Runbook timing table: QC sample vs structure vs off vs full; default remains `sample`.
- [ ] **DoD-2:** stderr includes `qc_ms=` on **both** QC-complete paths (ok and hard-fail). `unique-pst-export.md` has a `qc_ms` row. A test asserts the stderr/on_log line contains `qc_ms=`.
- [ ] **DoD-3:** `unique_pst_qc_0080` still source-differential at sample; clap default still `sample`.
- [ ] **DoD-4:** `cargo test -p pst-dedup-cli --test unique_pst_qc_0080` plus the stderr `qc_ms=` hook; fmt/clippy as gate.
- [ ] **DoD-5:** `review.md`; registry Completed; CHANGELOG; ledger **FEATURE** (stderr is user-visible).

## 8. Verification

```powershell
Set-Location C:\dev\Dedupe
cargo test -p pst-dedup-cli --test unique_pst_qc_0080
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 9. Deferred

| ID | Disposition |
|---|---|
| **D-0130-qc-sample-cost** | **Absorb — this track** (honesty; not speedup). |
| 0080 default sample | **Keep.** |
| Optional cheaper sample | **Decline this track** (mint later only with a no-contract cache). |
| **D-0079-operator-multigb** | **Decline** (not `--jobs`). |
| Bugbot #143–#146 | **Decline.** |

## 10. Unblocks

Operators can choose structure for timing passes without thinking the product “got slower.” Parallel with **0131** / **0132**.
