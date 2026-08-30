# 0113 — ProduceChecklist — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> **Ledger (implement):** `ledgerful ledger start crates/dedupe-chrome --category FEATURE --message "0113 produce checklist DAT-only chrome wizard"` — commit in the final phase.
>
> Planning tx (this Ready pass): `a0276927-0e11-49b2-8ceb-e12bd65b8207`.
> Fold-in tx: `97166878-d93e-479e-b75e-d78d232f3514` (`opencode-review.md` + `agy-review.md`; spec §2.10).

---

## Phase 0 — Precondition / pin gate → DoD-1

- [ ] Re-verify `SCHEMA_VERSION` 39, `run_produce`, `run_production_qc`, `check_qc_gate_for_pack`, `export_privilege_log`, `FilterSpec::preset_responsive` / `preset_withheld`, `list_items_filtered_thin`, `Matter::create_job`, `is_encrypted_matter` (spec §2.2). Confirm **no** `list_item_ids_filtered` / `count_produced_items` / `latest_control_number` yet — this track adds them.
- [ ] Re-verify volume layout is `DATA/load.dat` + `NATIVES/` + `TEXT/` and DAT `BEGBATES=ENDBATES=CONTROL_NUMBER`.
- [ ] Re-verify tauri **2.x stable** + leptos **0.8** (reject 3.x / 0.9-beta). Keep `ui/` workspace **exclude**.
- [ ] Confirm CI `chrome-ui` (wasm + trunk **0.21.14**) still present — do not drop it.
- [ ] Do **not** vendor `C:\dev\dedupe-frontend`. Do **not** add zpdf. Do **not** implement **0117** or **0118**.
- [ ] Do **not** depend on `process-runner` or `dedupe-desk`. Do **not** change engine defaults for `fail_if_withheld` / `require_qc_pass`.

## Phase 1 — Matter helpers + host commands → DoD-2, DoD-3, DoD-4, DoD-5

- [ ] `FilterSpec::preset_produce_responsive()` in `matter-core`.
- [ ] `Matter::list_item_ids_filtered`, `order_ids_family_together` (**first-occurrence family order**, not raw `family_id`), `count_produced_items` (`COUNT(DISTINCT)` + complete/`complete_with_errors` only), `list_production_sets_thin`, `latest_control_number` (same statuses; exclude `failed`; skip `SKIP_*`). `PrivilegeLogExportParams.control_numbers` map (ControlNumber + ParentControlNumber).
- [ ] Chrome deps: `matter-produce` + `matter-qc` (path crates). Still no `process-runner`.
- [ ] Host module `produce.rs` (name as you like): `produce_page` (read), `produce_qc_run` / `produce_start` (write + `create_job` + engine). `join_worker`. Encrypted first. Actor `"chrome"`.
- [ ] Chrome produce **and** QC params: `scope=item_ids`, `expand_family=false` / `expand_family_for_scan=false`, **same** `effective_qc_pack_id` from the selected profile, `fail_if_withheld=true`, `require_qc_pass=true`, required `bates_start`.
- [ ] Warning overrides: **session/payload** keyed `rule_id`+`item_id` against current findings; empty reason fails; `append_audit` after validate. **Do not** add an audit-query helper. `produce_start` re-resolves; membership drift → stale blocker; never silent re-QC.
- [ ] Chrome extras `uncoded_in_set` / `privilege_log_blank` evaluated on the host (not new QC pack rules).
- [ ] Extend `matter_overview` with `produced`. Fill `review_document.bates` from `latest_control_number`.
- [ ] Register commands + `allow-*`. Rebuild permission tomls. No `fs:default`.
- [ ] Unit tests (`tempfile` only) covering DoD-2..4: first QC has **both** `withheld_in_selection` and `uncoded_in_set`; warning override payload; volume layout; Bates on produced log rows + **withheld row ControlNumber=item_id**; window Bates; parent Bates < child; **two families keep input order**; no OPT.

## Phase 2 — Leptos wizard → DoD-1, DoD-3

- [ ] Replace `ProduceStub` on `/matters/:id/produce`. Five steps. Family-together locked. Page-level Bates control disabled (copy names **0115**). Format greys TIFF/OPT (**0115**). Burn copy names **0114**.
- [ ] Blocker cards (red) vs warning cards (override form). Finalize disabled while blockers or un-overridden warnings.
- [ ] Home chip uses `produced` (no `0113` subtitle **or** tooltip). Queue cell not `— · 0113`. Window Bates no `"0113"`.
- [ ] Plex/paper; no `#ec3013`. CSP unchanged. “Open in review” → 0112 route.

## Phase 3 — CI + docs → DoD-5

- [ ] `cargo fmt` / `clippy -D warnings` / `cargo test --workspace` / `cargo test -p dedupe-chrome` / `cargo test -p matter-qc` / `cargo test -p matter-produce` / `cargo check -p dedupe-desk` / trunk build `ui/`.
- [ ] CHANGELOG Unreleased. Close `D-0113-produce-checklist`. Leave **0114–0118** as-is. Note partial D-0040-04 / D-0031-09.

## Phase 4 — Finalize → DoD-6

- [ ] Owner HITL: **release** EXE. Synthetic 3-doc family: withheld blocker, warning override text required, clean DAT volume, home chip numeric. INC* waived.
- [ ] `review.md`; `../conductor.md` + `sequencing.md` + `ROADMAP.md`: **0113 Completed**.
- [ ] Commit the ledger transaction.
- [ ] **0115** still parked. **0117** / **0118** still Proposed.

---

## Handoff notes

- Planning-only until the user says **Implement**.
- Single-exe / no-daemon. Process jobs remain `dedupe-desk` until **0116**.
- Privilege-in-set is a hard block. Do not add a chrome bypass for `require_qc_pass`.
- Privilege log `filter_ids` = produced ∪ withheld-in-scope (never produced-only).
- Warning gate is the override **payload**, not an audit table read.
- Live layout is `DATA/load.dat`, not `DATA.dat`.
- Doc-level Bates this track (`BEGBATES=ENDBATES`). Page-level is **0115**.
- Do **not** implement 0117 / 0118 in this branch unless the user expands those tracks.
- `conductor/` is gitignored; `git add -f` track files when the owner commits.
- Never commit client PSTs, `output/`, or matter folders with mail.
