# 0110 — MatterChromeTauri — Plan

> Phased checklist; map each phase to `spec.md` §7. Execute in `C:\dev\Dedupe`.
> **Ledger:** `ledgerful ledger start crates/dedupe-chrome --category FEATURE --message "0110 Tauri 2 + Leptos matter chrome (one overview command)"` — commit in the final phase.
>
> Fold-in 2026-08-29: `opencode-review.md` + `agy-review.md` (see spec §2.10).

---

## Phase 0 — Precondition / pin gate → DoD-1

- [ ] Re-verify `SCHEMA_VERSION`, `load_case_overview_on`, `Matter::create` / `open_for_read` / `is_encrypted_matter` / `MatterInfo` / pub `insert_source` (spec §2.2). Re-verify tauri **2.x stable** and leptos **0.8** on crates.io. Reject any **3.x / pre-release** resolve of `tauri`.
- [ ] Confirm CI still `dtolnay/rust-toolchain@stable` (no nightly).
- [ ] Dev tools: `cargo install tauri-cli --locked --version "^2"` and `trunk` on PATH (record versions in `review.md`). CI may still residual `D-0110-ci-trunk`.
- [ ] Do **not** vendor `C:\dev\dedupe-frontend`. Do **not** add zpdf. Do **not** touch unique-pst.

## Phase 1 — Host crate + overview command → DoD-1, DoD-2, DoD-3, DoD-5

- [ ] Add `crates/dedupe-chrome` workspace member; `exclude` `crates/dedupe-chrome/ui`. `deny.toml` proprietary exception. Version `0.2.0-rc.1`.
- [ ] `matter_overview`: `is_encrypted_matter` first (never `open_*` if encrypted); else one `open_for_read` + `info` + `load_case_overview_on`. Chip map per spec §3.3. Blocking worker. No `unwrap`/`expect`.
- [ ] `create_matter`: `root = parent.join(validated_name)` then `Matter::create(&root, name)`.
- [ ] Recents: injectable dir; MRU-front; truncate tail to 20; missing roots remain listed.
- [ ] Unit tests (`tempfile` only): empty zeros; `insert_source` → Sources=1 **and** Processed=0; encrypted → kind `encrypted` (no open); invalid name; recents cap + order + inject dir (not LocalAppData); missing root → `not_found`; `<parent>/<name>/matter.db` after create. Encrypted test must not call `open_*`. If any test touches `PST_DEDUPE_MATTER_PASSPHRASE`, mutex + restore.

## Phase 2 — Leptos chrome → DoD-1, DoD-3, DoD-4

- [ ] `ui/` Leptos 0.8 CSR + Trunk 1420 + `withGlobalTauri`. Routes in spec §3.2.
- [ ] Encode/decode helper: percent-encoded `:id` round-trips a Windows path with spaces, `é`/`ü`, and `C:\…` back to the same root for `matter_overview`.
- [ ] Plex/paper tokens; self-hosted OFL woff2; no `#ec3013`. Skip links; `:focus-visible`; `Ctrl+K`.
- [ ] Matter list + home chips from `matter_overview`. Stubs for process/review/produce/admin. Produced = `—`.
- [ ] Window 1440×900, title `Dedupe Desk`. CSP object per spec §3.6 (`'wasm-unsafe-eval'`, IPC `connect-src`, no Google Fonts). Host test: `tauri.conf.json` parses and `script-src` contains `'wasm-unsafe-eval'`.

## Phase 3 — CI + docs → DoD-5

- [ ] `cargo fmt` / `clippy -D warnings` / `cargo test --workspace` / `cargo check -p dedupe-desk`.
- [ ] CI: wasm target + `trunk build` for `ui/`, **or** `D-0110-ci-trunk` P3 if blocked (host tests still required).
- [ ] CHANGELOG Unreleased. Short `ARCHITECTURE.md` pointer to `crates/dedupe-chrome`. Close `D-0110-matter-chrome`.

## Phase 4 — Finalize → DoD-6

- [ ] Owner HITL: launch **release** EXE (CSP is enforced on bundled assets — a `devUrl` pass is not enough). Synthetic matter; chips match empty Desk Overview. If white-screen, check `'wasm-unsafe-eval'` first. Note in `review.md`. INC* waived.
- [ ] `review.md`; `../conductor.md` + `sequencing.md` + `ROADMAP.md`: **0110 Completed**.
- [ ] Commit the ledger transaction.
- [ ] Unblocks **0111** and **0113**. **0115** stays parked.

---

## Handoff notes

- Planning-only until the user says **Implement**.
- Single-exe / no-daemon. Process jobs remain `dedupe-desk` until **0116**.
- Do not fake Produced=0. Do not open encrypted matters.
- `conductor/` is gitignored; `git add -f` track files when the owner commits.
- Never commit client PSTs, `output/`, or matter folders with mail.
