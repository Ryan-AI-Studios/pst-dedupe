# 0109 — AlsoEmlClassifyHonesty — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> **Ledger:** `ledgerful ledger start pst-dedup-cli --category BUGFIX --message "0109 also-eml classify/cancel honesty (PR #104 Bugbot)"` — commit in the final phase.

---

## Phase 0 — Precondition / diagnosis gate → DoD-1

- [ ] Re-read `unique_pst_cmd.rs` combined classify (~3340–3427), summary rewrite (~3536–3577), JSON `AlreadyEmitted` (~3649); `unique_eml_cmd.rs` cancel `Err`→`Ok` (~449–471) + `also_eml_recovered_counts`; `export_outcome.rs` `classify_export` / `worse_cli_exit` / `summary_is_retryable`; `docs/unique-pst-export.md` `ok == (fidelity == complete)`.
- [ ] Confirm Bugbot sites still live (no silent main fix). Re-verify line numbers.
- [ ] Do **not** rewire `--also-eml`. Do **not** edit 0108 poly rate. Do **not** touch frontend.

## Phase 1 — Helpers → DoD-1, DoD-2

- [ ] `worse_export_fidelity` in `export_outcome.rs` (Failed > Partial > Complete).
- [ ] `finalize_unique_pst_classify(pst, eml_fidelity: Option<ExportFidelity>)` — **fidelity worse-of only**. `None` leaves PST fidelity. Do **not** take EML exit/reasons/cancelled. Do **not** call `worse_cli_exit`. Do **not** change `artifact_state`. `ok` is computed at the unique-pst call site after this helper.
- [ ] `classify_after_summary_write_failure(..., process_cancelled)`: if cancelled → `classify_export(..., cancelled=true)`; else `report_ok=false` then classify.
- [ ] Unit tests in spec §3.5 helper rows (inject `ExportFidelity::Partial` / `Failed`; Complete+Some(Complete) stays Complete for the 65 case). Do **not** assert exit integers inside the fidelity helper.

## Phase 2 — Wire unique-pst + unique-eml → DoD-1, DoD-2, DoD-3

- [ ] Add `fidelity: ExportFidelity` to `WriteEmlPackFromKeepSetResult`. Inner Ok copies classified fidelity; cancel `Err`→`Ok` sets Failed.
- [ ] Cancel `Err`→`Ok`: fill counts from `also_eml_recovered_counts` (not zeros).
- [ ] unique-pst: **delete** the `match combined_exit` fidelity rewrite. After the existing exit/reason merge (~3353–3415), call `finalize_unique_pst_classify` with `Some(pack.fidelity)` when also-eml ran/cancelled, else `None`. Then `ok = (classified.fidelity == Complete) && !process_cancelled`. Do **not** merge exit/reasons a second time.
- [ ] Summary rewrite: pass `process_cancelled` into `classify_after_summary_write_failure` (and `summary_is_retryable`).
- [ ] Combined exit merge (~3352–3383) stays 0078 precedence. PST `artifact_state` stays pre-merge.
- [ ] Clippy `-D warnings`; no production `unwrap`/`expect`.

## Phase 3 — Recovery test + docs → DoD-3, DoD-4, DoD-5

- [ ] `cancel_ok_recovers_attach_and_embedded_from_summary`: seed `{out}/summary.json` as a **file** (`eml_written=7`, `attach_parts_failed=2`, `embedded_messages_written=3`); `fs::create_dir_all({out}/manifest.json)` (directory collision — same Err as `helper_hard_fail_writes_summary_json`); `cancel=true`. Assert recovered **7/2/3**, not zeros. Existing blocked-**summary** 130 test stays (counts may be 0).
- [ ] Additive sentences in `docs/unique-pst-export.md` JSON fields (spec §2.3 / §3.6): combined job vs PST `artifact_state`; allow-partial now emits `error.code=partial_fidelity` with `ok=false`. CHANGELOG Unreleased.
- [ ] Close `D-0109-also-eml-classify`.

## Phase 4 — Finalize → DoD-6

- [ ] `review.md`: results, test names, Bugbot IDs closed, deferred leftovers.
- [ ] `../conductor.md` + `sequencing.md` + `ROADMAP.md`: **0109 Completed**.
- [ ] Commit the ledger transaction.
- [ ] Unblocks: honest `--also-eml` automation; Series O **0110+** still next frontend.

---

## Handoff notes

- Planning-only until the user says **Implement**.
- 0107 isolation lock remains: also-eml failure/cancel does not quarantine PST volumes.
- Cancel during PST write still skips also-eml (`also_eml_ran=false`) — do not change.
- Schema ids not bumped. No `also_eml_fidelity` key.
- Single-exe / no-daemon unchanged.
- Never commit client PSTs or `output/`.
