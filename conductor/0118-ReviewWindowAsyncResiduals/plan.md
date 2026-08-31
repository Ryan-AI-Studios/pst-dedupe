# 0118 — ReviewWindowAsyncResiduals — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Ledger:** `ledgerful ledger start 0118-review-window-async --category BUGFIX --message "Stale review fetch guard + same-item codes/notes refresh + path_id test"`

---

## Phase 0 — Precondition / API gate → DoD-4

- [ ] Re-verify `SCHEMA_VERSION == 41`. Re-read live `ui/src/pages/review_window.rs` (document Effect ~290, body Effect ~357, persist ~662–670 including `then_next && next is None`, raster gen ~403). Re-read `ui/src/path_id.rs` tests (~111–138).
- [ ] Re-read PR #115 comments `d4f586ec`, `e7aae96b`, `ef6ecfe4`.
- [ ] Do **not** implement 0119–0122. Do **not** bump schema. Do **not** edit `review_window_apply` host sequence or Image-tab overlay handlers.

## Phase 1 — Fetch guard → DoD-1

- [ ] Add `fetch_is_current` + unit tests in the ui crate.
- [ ] `doc_generation` / `body_generation`: increment before `spawn_local`; apply OK/Err **and catalog** only when current (body also `pane`).
- [ ] Leave raster `raster_generation` as-is.

## Phase 2 — Same-item refresh + path_id + CI → DoD-2, DoD-3

- [ ] After persist success that **does not navigate** (`then_next && next_id is None`; also `then_next == false` if added): increment `doc_generation`, re-invoke `review_document` under `fetch_is_current`; refresh codes/notes/privilege + `pending_*`. Refresh fail → `status` only, keep `doc`.
- [ ] Persist that **does** navigate: `go_item` only.
- [ ] One `#[test]` on `review_doc_href_encodes_filter_and_keyword`; `#[test]` on `stub_back_href_reencodes_decoded_windows_param`.
- [ ] chrome-ui job: `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml`.

## Phase 3 — Verify → DoD-4

- [ ] `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml`
- [ ] `cargo test -p dedupe-chrome`
- [ ] trunk / chrome-ui still builds. Review / Process / Produce / Queue still route.

## Phase 4 — Finalize → DoD-5

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (or ledgerful verify --scope full)
- [ ] CHANGELOG Unreleased sentence.
- [ ] Write `review.md` (commands, HITL rapid next + Enter-on-last-item un-privilege).
- [ ] Update `../conductor.md` → **Completed**. Close `D-0118-review-window-async` in `docs/deferred.md`.
- [ ] Commit the BUGFIX ledger transaction.
- [ ] Owner HITL: release EXE, synthetic 3-doc family.

---

## Handoff notes

- Single-exe / no-daemon. Unique-pst is **not** this page.
- **0119–0122** stay Proposed. Do not steal Image-tab draw (**0120**) or cancelled-produce (**0119**).
- `conductor/` new files need `git add -f` when the owner commits (directory is gitignored for untracked).
