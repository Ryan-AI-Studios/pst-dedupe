# 0129 — AlsoEmlPhaseTimings — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Ledger (execute):** `ledgerful ledger start 0129-also-eml-ms --category FEATURE --message "phase_timings.also_eml_ms additive; never fudge unaccounted"`
>
> **Fold-in 2026-09-03:** opencode-M1/m1/m2/O2 + AGY-129-01..03.

## Phase 0 → DoD-4

- [ ] Re-read `PhaseTimings` / `accounted_ms` / `finalize` and the `stage=also_eml` block in `unique_pst_cmd.rs`.
- [ ] Do **not** force `unaccounted_ms` to 0. Do **not** add `--jobs`. Leave `qc_ms` docs row to **0130**.

## Phase 1 → DoD-1, DoD-2, DoD-3

- [ ] Add `also_eml_ms` (`#[serde(default)]`) and include it in `accounted_ms()`.
- [ ] `let t_also_eml = Instant::now();` **before** the `write_eml_pack_from_keep_set` match; assign elapsed **after** the match (Ok / cancelled / Err). Flag-off stays 0; skip-before-block stays 0.
- [ ] Docs: `also_eml_ms` row after `verify_ms` / near `quarantine_ms`. Coordinate with 0130 if same release.
- [ ] `tests/unique_pst_also_eml.rs`: `also_eml_ms > 0` when ran; `0` when omitted. Unit: `accounted_ms` includes the field.

## Phase 2 → DoD-4, DoD-5

- [ ] `cargo test -p pst-dedup-cli --test unique_pst_also_eml` (not `phase_timings`)
- [ ] fmt / clippy / CHANGELOG / `review.md` / registry Completed / ledger commit
