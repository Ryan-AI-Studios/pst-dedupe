# 0119 — ProduceChecklistResiduals — Review

## Scope

Produce wizard honesty after PR **#117** / **#123** Bugbot: empty privilege-log `filter_ids`, Finalize re-arm / colliding Bates, QC leak across matters, cancelled/idle treated as success. Schema stays **41**. **0120–0126** product code not implemented. Five-step wizard unchanged (**0125**).

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Empty filter | PASS | `apply_privilege_log_id_filter`: `None` no-op, `Some([])` `AND 0`, else `IN`. `export_privilege_log` + `count_privilege_log_blank_descriptions` share it. Tests: `export_csv_two_items_headers` (`Some([])` 0 rows; `None` still 2); `empty_filter_ids_does_not_count_corpus_blanks`; host `empty_union_privilege_log_blank_is_zero_not_corpus`. Chrome still `Some(union)`. |
| DoD-2 Second Finalize disabled | PASS | Dedicated `volume_succeeded` latch set on produce `succeeded` (before `produce_page` refresh). Finalize `disabled` uses `finalize_blocked_by_volume_latch`; click no-ops when latched. QC `start_result.set(None)` does not clear the latch. `next_seq_hint` → `bates_start` only on that success refresh. Genuine privilege-log post-step errors keep `state==succeeded` (`apply_privilege_log_post_step`) and surface `privilege log: …` without re-arming. UI tests: latch helpers + `finalize_view_wires_latch_not_start_result_ok`. Owner HITL remaining. |
| DoD-3 Matter switch | PASS | Route Effect clears `qc` / `overrides` / `start_result` / `volume_succeeded` / `step` / `entire_corpus` / `bates_start` / banners. Does **not** reset `start_busy` / `qc_busy`. After every await, ignore if captured root ≠ `root_sig`. |
| DoD-4 Cancel / idle | PASS | `process_job_succeeded` is exact `"succeeded"`. Produce/QC waits do not treat `cancelled` / `idle` / `paused` / `failed` as volume-ok or load QC findings. Runner `Paused` → `paused` / `"cancelled"` unchanged. |
| DoD-5 Hygiene | PASS | No new production `unwrap`/`expect`. Schema 41. `dod2_default_set_privilege_in_set_and_uncoded` passed. Privilege tests 19; chrome-ui 18; wasm check passed. |
| DoD-6 Recorded | PASS | This file; registry **Completed**; `D-0119-produce-checklist-residuals` closed. Ledger BUGFIX `02221f56` committed on the product squash. Residual **D-0060-03** first-paint Bates auto-fill. |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` | 18 passed |
| `cargo test -p matter-core --test privilege` | 19 passed |
| `cargo test -p dedupe-chrome` (focused + full lib 110) | pass |
| `cargo check --target wasm32-unknown-unknown` (ui) | pass |
| `cargo test --workspace` | pass (~330s) |
| `ledgerful verify` | pass (pre-push: fmt + clippy + workspace 277.9s) |
| CI (PR **#129**) | fmt, clippy, test, audit, deny, chrome-ui, verify-parity **green**. Bugbot NEUTRAL (skipping). |
| Codex r3 | **PASS**, no findings |

## Reviewer rounds

1. Internal: DoD-1…4 wired; schema / runner Paused / privilege-in-set untouched.
2. Codex r1: FAIL — P1 missing two-click wiring proof; P2 gates/DoD-6 (process). Wiring `include_str` test added; gates completed; DoD-6 deferred to this closeout.
3. Codex r2: FAIL — P1 genuine privilege-log post-step `?` failed the progress poll so the latch never set. Fixed: `apply_privilege_log_post_step` keeps `succeeded` and attaches `error_summary`.
4. Codex r3: **PASS**. Fresh pass; no open >low.

## HITL (owner)

Release chrome EXE, synthetic produceable set: Finalize once → second Finalize must not write the same Bates; switch matter → QC cards gone; cancel from Process while producing → wizard must **not** show a completed volume. Codesign is **D-0062-codesign**. INC* unique-pst is not a gate.

## Publish

- Branch: `track/0119-produce-checklist-residuals`
- PR: **#129**
- Merge SHA: `6a775b504317989ed6e11b3f08153a6bffb9a81e`
