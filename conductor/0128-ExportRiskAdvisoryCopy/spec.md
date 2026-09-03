# 0128 — `export_risk` copy when the unique is complete

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Do **not** discount `BODY_UNAVAILABLE` from `effective_degraded_winner_rate` (0108 lock).
> Do **not** restrip keep-set `CRC_SUSPECT` (`D-0108-keepset-crc-retaint` never-mint).
> Do **not** add a fourth `export_risk` value. Threshold `max_degraded_winner_rate` **0.02** stays.
> `--fail-on-export-risk` vocabulary stays `ok` \| `re_export_recommended` \| `not_export_ready`.
> No BCC-default. Not frontend. Do not steal **0100–0104**.

- **Track ID:** 0128-ExportRiskAdvisoryCopy
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** unique-pst export_risk honesty. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-03).
- **Status:** Ready — not started
- **Depends on:** **0108 Completed** (`effective_degraded_winner_rate`)
- **Spec authored:** 2026-09-03 (placeholder → Ready)
- **Series:** U (unique-export INC* HITL residuals)
>
> **Closes / absorbs:** `D-0128-export-risk-advisory-copy` and the 0108 HITL leftover (“INC* stays advisory if body-unavailable share > 0.02”).
> **HITL:** 2026-09-02 operator-local INC* — exit 0, fidelity `complete`, 4055/4055, attach fails 0; `export_risk.level=re_export_recommended` because `effective_degraded_winner_rate=0.031>0.02` (124 `BODY_UNAVAILABLE`; 3931 poly-only discounted). Pack `output/inc0102784-post-0126/` gitignored. Never commit those PSTs. CI uses unit fixtures, not INC*.
>
> **Harness fold-in (2026-09-03):** `opencode-review.md` + `agy-review.md`. `body_unavailable_winners` on `ExportRiskInputs`; helper `(fidelity, &ExportRisk)`; note only when keyed degrade is the sole elevating cause; `emit_log` not `println!`; serde skip none. See §2.9 / §9.

---

## 1. Objective

A structurally **complete** unique-pst job must not *read* like a failed export. 0108 stopped the 1.000 poly-CRC lie; INC* still trips advisory because ~3% of winners have no body. Operators (and `--fail-on-export-risk`) need copy that says **complete + body-unavailable / non-poly degrade share**, not “this unique is junk” and not “re-export the Permute store for CRC.”

This is **disclosure honesty**, not a threshold change. Missing bodies stay on the keyed rate.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

0108 keyed `effective_degraded_winner_rate` so poly-only CRC no longer inflates the advisory. Runbook §5 still says prefer Purview re-export whenever `export_risk` is `re_export_recommended`. That sentence is wrong for a complete unique whose only post-export trip is non-poly `BODY_UNAVAILABLE` share on a Purview dump.

### 2.2 Live APIs (plan-time 2026-09-03, HEAD `f8cb240`; re-verify at execute)

| Surface | Fact |
|---|---|
| Schema | **41**. N/A this CLI track (no bump). |
| `ExportRisk` (`unique_export_report.rs`) | `level`, closed-vocab `reasons`, `inputs`, `thresholds`. **No** `operator_note`. |
| `compute_export_risk_with_thresholds` | Keys `effective_degraded_winner_rate` vs `max_degraded_winner_rate` **0.02** → reason `effective_degraded_winner_rate=0.031>0.02`. Attest `poly_class_crc_discounted` may co-occur. Thresholds cannot alone reach `not_export_ready`. |
| `ExportRiskInputs` | Raw + effective degrade rates; `degraded_winners_poly_only`. **No** dedicated `body_unavailable` count on this struct. |
| `--fail-on-export-risk` | Optional; gate ranks `ok` / `re_export_recommended` / `not_export_ready`; exit **65**. Do not change the gate. |
| Runbook §5 | “Prefer Purview re-export when `re_export_recommended`” — undifferentiated. |
| Desk wizard | `unique_wizard.rs` may print `re_export_recommended` without body copy. **Out of scope** (D-0078-gui residual). |

### 2.3 Pins

CLI on stable Rust. No schema bump. No daemon. Reasons stay closed-vocab (numeric rate strings + attest tokens). Do not invent `ok_with_bodies_missing`.

### 2.4 Tools (plan-time)

`ai-brains preflight` inited; ledger 0 pending / 0 drift; `scan --impact` LOW (conductor docs). Federated `output/` budget — ignore INC* packs.

### 2.8 Last-PR Cursor comments

PRs **#146, #145, #144, #143** (last four merged product PRs at plan-time): inline comments **0**, reviews **0**, Bugbot usage-limit only on #145. **Decline** — nothing to fold.

### 2.9 Product locks

- Additive `operator_note: Option<String>` on `ExportRisk` with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- Additive `body_unavailable_winners: u64` on `ExportRiskInputs` with `#[serde(default)]`. Populate from keep-set winners (`IntegrityReason::BodyUnavailable` / write_msg flag) **before** `compute_export_risk`. Helper does **not** take `KeepSet`.
- Helper: `export_risk_operator_note(fidelity, &ExportRisk) -> Option<String>`. `Some` only when:
  - fidelity is **complete**, and
  - `risk.level == ReExportRecommended`, and
  - the **sole** elevating post-export/scan reasons are keyed degrade (`effective_degraded_winner_rate=…` or `degraded_winner_rate=…`) plus optional attest `poly_class_crc_discounted`.
  - **None** if `attach_fail_rate=…`, `failed_volume_index`, scan `not_export_ready` / `scan_preflight=re_export_recommended`, `attach_stream_crc_events=…`, or keyed block-CRC reasons are present.
- Note text names **BODY_UNAVAILABLE / non-poly degrade** using `inputs.body_unavailable_winners` and the keyed rate — not “100% CRC.” Keep raw + effective rates on `inputs`.
- stderr: `emit_log(stderr, &on_log, &format!("note: {note}"))` **before** the Phase 6 stdout/`--json` branch. Never `println!` the note (human `export_risk:` line at ~3700 is stdout; do not put the note there).
- Runbook **§5 first** (undifferentiated “prefer Purview re-export” lives there). Touch `unique-pst-export.md` only if execute finds it still equates advisory with re-export (plan-time: CRC table already distinguishes poly vs real degrade).
- Do **not** raise 0.02. Do **not** discount `BODY_UNAVAILABLE`. Do **not** restrip keep-set CRC.

---

## 3. In scope

`pst-dedup-cli` `ExportRisk` note + stderr via `emit_log`; unit tests next to existing 0108 poly-only cases; `docs/unique-pst-ediscovery-runbook.md` §5. `unique-pst-export.md` only if still undifferentiated.

## 4. Out of scope

Discounting bodies. Soft-body recovery (`D-0066-soft-body`). Keep-set CRC restrip. Fourth risk enum. Raising 0.02. Desk wizard copy. Frontend. BCC-default. 0127 / 0129–0132.

## 5. Preconditions

0108 effective rate shipped. INC* numbers are operator-local evidence, not CI.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Softening copy so counsel ignores missing bodies | Keep numeric rate; only clarify *why* |
| New risk enum breaks `--fail-on-export-risk` | Forbidden — copy/note only |
| Operators re-export Permute for poly CRC | Runbook split: poly vs bodies |
| Counting BODY_UNAVAILABLE without an inputs field | **Folded:** `body_unavailable_winners` serde-default on `ExportRiskInputs`; helper stays `(fidelity, &ExportRisk)` |
| Note firing beside attach-fail / scan not-ready | Helper requires keyed degrade as the **sole** elevating cause |
| `println!` note on `--json` stdout | `emit_log` only |

## 7. Definition of Done

- [ ] **DoD-1:** Fixture: complete + poly-only (effective degrade 0) → `operator_note` absent. Complete + keyed degrade > 0.02 with `attach_fail_rate` ≤ 0.05 and `failed_volume_index` None → note/stderr names BODY_UNAVAILABLE / non-poly degrade (uses `body_unavailable_winners`), not “100% CRC.” Complete + keyed degrade **and** attach-fail spike → note **absent**. Closed-vocab `reasons` still include the rate string.
- [ ] **DoD-2:** Runbook §5: do not re-export Permute for poly CRC; inspect missing bodies; gate vocabulary frozen. Export-docs only if still undifferentiated.
- [ ] **DoD-3:** `--fail-on-export-risk re_export_recommended` still exits 65 on this advisory (no silent pass). Threshold 0.02 unchanged.
- [ ] **DoD-4:** `cargo test -p pst-dedup-cli` covering `compute_export_risk` / operator_note helper; fmt/clippy as gate.
- [ ] **DoD-5:** `review.md`; registry Completed; CHANGELOG; ledger FEATURE.

## 8. Verification

```powershell
Set-Location C:\dev\Dedupe
cargo test -p pst-dedup-cli export_risk
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 9. Deferred

| ID | Disposition |
|---|---|
| **D-0128-export-risk-advisory-copy** | **Absorb — this track.** |
| 0108 HITL “still advisory if body-unavailable > 0.02” | **Absorb** (copy, not threshold). |
| **D-0108-keepset-crc-retaint** | **Decline** (never-mint). |
| **D-0066-soft-body** | **Decline** (no partial-byte recovery). |
| Bugbot #143–#146 | **Decline.** |
| Desk wizard banner | **Decline** (not this CLI track). |
| opencode-O2 pinned-count trivia | **Decline.** |

## 10. Unblocks

Counsel-facing unique can stay `re_export_recommended` without looking like a failed write. **0129** clocks are independent.
