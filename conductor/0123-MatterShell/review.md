# 0123 — MatterShell — Review

## Scope

Shared 46px TopBar + 30px StatusBar on every matter workspace route (Home, Process, Review queue, Review window, Produce, Admin). Home sits under that bar after Open (not a fifth tab). Admin is an inert span. Recents JSON with a UTF-8 BOM loads. Schema stays **41**. **0122** Busy helpers untouched. **0124** rail/Go-to, **0125** produce canvas, and **0126** jobs table not implemented.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Shared TopBar | PASS | `app.rs` launcher `.top-bar` only on `/matters`. Matter routes wrap `MatterShell` (`ui/src/shell.rs`). Brand `Dedupe Desk`, shared `matter_overview`, Process/Review/Produce hrefs under `/matters/{id}/…`, Admin `<span class="workspace-tab-inert">`. `wrap_review_window` passes `WorkspaceTab::Review`. In-page `nav.tabs` / `← Matter home` removed. `← Queue` kept. Owner HITL remaining (release EXE). |
| DoD-2 StatusBar | PASS | 30px paper + 2px ink (`app.css`). Process deterministic sentence is `PROCESS_FLAG` in the shell, not Process body. Produce flag is the privileged-document override rule. Host `process_ui_is_live_not_stub` `include_str!`s `ui/src/shell.rs`. |
| DoD-3 Home + list | PASS | Open still navigates to `/matters/{id}`. Home reuses `MatterShellCtx` (one overview fetch). Placeholder empty sentence deleted. Matters list has no workflow tabs. |
| DoD-4 Recents BOM | PASS | `strip_utf8_bom` before `from_str`. Tests: BOM load, write has no `EF BB BF`, corrupt-after-strip still errors. |
| DoD-5 Hygiene | PASS | No new production `unwrap`/`expect`. Schema 41. Plex + `--action: #1b3049`. No Archivo/coral/`DEDUPE / REVIEW`. 0122 Busy tests still pass. `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` (30) + `cargo test -p dedupe-chrome` + workspace + clippy `-D warnings`. chrome-ui CI trunk build green. |
| DoD-6 Recorded | PASS | This file; registry **Completed**; `D-0123-matter-shell` closed. Ledger FEATURE `13674ee3` committed on the product squash. |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` | pass (30, including Busy + `shell_source_locks`) |
| `cargo test -p dedupe-chrome` | pass (recents BOM + `process_ui_is_live_not_stub`) |
| `cargo test --workspace` | pass |
| `ledgerful verify` | pass |
| CI (PR **#139**) | fmt, clippy, test, audit, deny, chrome-ui, verify-parity **green**. Bugbot NEUTRAL (does not block). |
| Final cross-model gate | **CLEAN**, no open >low |

## Reviewer rounds

1. Internal: DoD-1…5 wired; two-bar risk mitigated by moving launcher chrome inside Router; Admin span; Review tab explicit on `:docId`. Easy P3: matter-name `aria-current` on Home. **PASS** (no >low).
2. Codex-style r1: **CLEAN** for engineering DoD-1–5 (DoD-6 left for this file).
3. Final gate (fresh): **CLEAN**.

## HITL (owner)

Release chrome EXE on a synthetic matter: Open lands on **Home under the shared bar** (not a deep-link to Process). Process, Review, Produce, and Admin all show the **same** 46px TopBar (four tabs; Admin inert span) and 30px StatusBar. Produce is no longer `← Matter home` with no tabs. Review tab stays active in the window. Recents JSON with a UTF-8 BOM loads. INC* unique-pst is not a gate. Codesign is **D-0062-codesign**.

## Publish

- Branch: `track/0123-matter-shell`
- PR: **#139**
- Merge SHA: `fce416ef1a4a6c861be19ecc113aeb235064d6e9`
