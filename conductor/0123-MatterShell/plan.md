# 0123 — MatterShell — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Ledger (execute):** After owner git-commits this Ready spec/plan if still dirty, `ledgerful ledger start 0123-matter-shell --category FEATURE --message "Shared matter TopBar+StatusBar; Home under bar; recents BOM-safe"`

---

## Phase 0 — Precondition / API gate → DoD-5

- [ ] Re-verify `SCHEMA_VERSION == 41`. Re-read live `ui/src/app.rs`, `pages/home.rs`, `process.rs` toolbar + `STATUS_BAR`, `queue.rs` tabs, `produce.rs` toolbar, `admin.rs`, `review_window.rs` toolbar, `src/recents.rs`, `ui/styles/tokens.css` + `app.css`. Re-read mock `top_bar.rs` / `status_bar.rs` / `app.css` 46/30 (research only).
- [ ] Confirm 0122 Busy helpers in `process.rs` are **not** edited. Confirm last PRs (#138–#135) still have no product findings.
- [ ] Re-read spec §2.9 locks (Plex, `#1b3049`, Home = brand/name not fifth tab, Admin span, no Archivo/coral).
- [ ] Do **not** implement 0124–0126 guts. Do **not** bump schema. Do **not** change `process-runner` Busy.
- [ ] Owner git-commits tracked conductor/docs **before** the product FEATURE tx. Do **not** `git add` repo-root `agy-review.md` or `fixtures/keep_set_summary.json`.

## Phase 1 — Recents BOM → DoD-4

- [ ] `strip_utf8_bom` (leading `\u{feff}`) before `from_str` in `recent_matters_list_in`.
- [ ] Unit tests: BOM-prefixed JSON loads; `remember` write bytes are not `EF BB BF…`. Corrupt-after-strip still errors.

## Phase 2 — Shared shell → DoD-1, DoD-2, DoD-3

- [ ] **`app.rs` (required):** move chrome inside `<Router>` (nested parent+`Outlet` or route-conditional header). Global dark `.top-bar` must **not** remain on matter routes. Launcher header only on `/matters`. Keep skip-links + `#main-content`. Keep `#ctrl-k-hint` persistently mounted.
- [ ] Add ui `TopBar` / `StatusBar` / `MatterShell` (names as fitted). Matter grid `46px 1fr 30px`. **One** `matter_overview` in the shell when `:id` changes; Home chips reuse it. On fail: last-segment name, omit processed/meta.
- [ ] Tabs: Process / Review / Produce `href=format!("/matters/{encoded_id}/…")` — never mock `"/process"`. Admin **span** (no hover-as-link). Brand + matter name → Home. On Home, no tab `active`. Review window passes `Tab::Review` (same as queue).
- [ ] Wrap Home, Process, Review, Review window, Produce, Admin. Matters list stays **outside**. Remove in-page workspace nav and `← Matter home` from those pages. Keep `← Queue` on the window. Keep `← Matters` as leave-matter (list).
- [ ] Move Process `STATUS_BAR` to StatusBar `.flag`; delete body copy. Update `process_ui_is_live_not_stub` `include_str` to the shell file if the sentence leaves `process.rs`.
- [ ] Produce / Review / Admin / Home flags per spec §2.9. Right TopBar slot reserved (flex; collapse when empty; optional Process progress readout; no Go-to; no avatar).
- [ ] Delete Home placeholder empty sentence. Keep chips + CTAs.
- [ ] Shell CSS: 0-radius / 2px ink on topbar/tabs/statusbar; action `#1b3049`; StatusBar **paper + ink text** (not mock dark strip); keep Plex. Do not port Archivo/coral.

## Phase 3 — Verify → DoD-5

- [ ] `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml`
- [ ] `cargo test -p dedupe-chrome` (recents + `process_ui_is_live_not_stub` + 0122 Busy tests)
- [ ] trunk / chrome-ui still builds.

## Phase 4 — Finalize → DoD-6

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (or ledgerful verify --scope full)
- [ ] CHANGELOG Unreleased sentence.
- [ ] Write `review.md` (commands, HITL: Open→Home under bar; **one** TopBar not stacked; Produce has tabs; Review tab stays active in the window; Admin span; BOM recents).
- [ ] Update `../conductor.md` → **Completed**. Close `D-0123-matter-shell` in `docs/deferred.md`.
- [ ] Commit the FEATURE ledger transaction.
- [ ] Owner HITL: release EXE.

---

## Handoff notes

- Single-exe / no-daemon. Unique-pst is **not** this page.
- **0124** Go-to / row range stay Proposed (slots reserved). **0125** canvas Proposed. **0126** jobs table Proposed. **0122** Busy stays Completed.
- 0123 `spec.md` / `plan.md` may already be tracked. `git add -f` only if `git status` shows them **untracked**.
- Do not `git add` stray repo-root `agy-review.md` or `fixtures/keep_set_summary.json`.
