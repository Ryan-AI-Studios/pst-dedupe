# 0108 — PolyDegradedWinnerRisk — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> **Ledger:** `ledgerful ledger start pst-dedup-cli --category BUGFIX --message "0108 effective degraded_winner_rate for poly CRC"` — commit in the final phase.

---

## Phase 0 — Precondition / diagnosis gate → DoD-1

- [ ] Re-read `compute_export_risk` degrade-rate branch, `ExportRiskInputs`, `poly_crc_risk_adjustment`, `unique_pst_cmd.rs` success + cancel `compute_export_risk` calls, `KeepEntry.integrity`, `FileScanStats.poly_class_crc`, `path_compare_key`, `compare_integrity_counters`, `SUMMARY_ALLOWLIST_KEYS`.
- [ ] Confirm `export_risk_all_poly_inc_like_ok` still defaults `degraded_winner_rate=0` on HEAD (the gap).
- [ ] Do **not** restrip keep-set. Do **not** edit 0109 also-eml classify. Do **not** touch frontend.

## Phase 1 — Helper + threshold keying → DoD-1, DoD-2

- [ ] Add `DegradedWinnerRiskAdjustment` + `poly_degraded_winner_adjustment` in `unique_export_report.rs` (spec §3.1–3.2).
- [ ] Extend `ExportRiskInputs` with `effective_degraded_winner_rate` / `degraded_winners_poly_only`: `#[serde(default)]` **and** the **manual** `impl Default` (`None` / `0`). Do **not** `skip_serializing_if`.
- [ ] Key the 0.02 advisory on effective-if-Some else raw. Emit degrade-rate reasons **only** in the `post == Ok` branch (match live raw; do **not** add them to the catastrophic `else`). Prefix `effective_degraded_winner_rate=` vs `degraded_winner_rate=`; never emit the raw 1.000 lie when effective is `Some`.
- [ ] Unit tests in spec §3.7. **Update** `export_risk_all_poly_inc_like_ok` by **mutating** the `inputs_from_sources` return (do **not** change that helper’s signature). Include `{AttachStreamCrc}` poly/non-poly, `{CrcMismatch}` fail-closed, `unique==0` both sides, `\\?\` + case-differ path match, scaled 39+2 → `0.049`. Existing 0099 block-rate tests stay green.

## Phase 2 — unique-pst wire-up + oracle → DoD-3, DoD-4

- [ ] Success path: after `crc_adj`, call the helper with `keep_set.winners` + `outcome.summary.files` + `crc_adj.poly_class_crc_discounted`. Raw rate still from `stats.degraded_winners`.
- [ ] Cancel path: leave effective `None` / poly_only 0.
- [ ] Oracle pointer list §3.5. **No** allowlist entry. No `keep_set_v1` / report schema id bump.
- [ ] Clippy `-D warnings`; no production `unwrap`/`expect`.

## Phase 3 — Docs + deferred → DoD-5

- [ ] Additive rows in `docs/unique-pst-export.md` CRC table (~294–309) and `docs/unique-pst-ediscovery-runbook.md` integrity table (~185–198) (spec §3.8). There is no “never discount `degraded_winner_rate`” sentence in those files — amend deferred/0099 instead.
- [ ] Close `D-0108-poly-degraded-winner-risk`. **Update** existing `D-0108-keepset-crc-retaint` (do **not** add a second row). CHANGELOG Unreleased.
- [ ] Optional HITL (DoD-7): INC* to `output/inc0102784-post-0108/` — never commit.

## Phase 4 — Finalize → DoD-6

- [ ] `review.md`: results, test names, HITL if run, deferred leftovers.
- [ ] `../conductor.md` + `sequencing.md` + `ROADMAP.md`: **0108 Completed**.
- [ ] Commit the ledger transaction.
- [ ] Unblocks: honest INC* risk banner; **0109** still next for also-eml classify; Series O **0110+** unchanged.

---

## Handoff notes

- Irreversible: none (report-only policy). Rollback = revert the keyed rate (keep-set bytes unchanged).
- Single-exe / no-daemon unchanged.
- Do not invent a fourth `export_risk` value. Do not raise 0.02. Do not promise INC* `ok` (HITL effective ≈ 0.031).
- Implementer: sample gitignored `keepset.json` only for local debug; never copy it into fixtures.
