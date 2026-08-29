# Track Completion Audit — 0110-MatterChromeTauri

## Verdict: FAIL

## Scope Reviewed

Read-only audit of branch `track/0110-matter-chrome-tauri`, including `spec.md`, `plan.md`, prior r1/r2 reviews and fixes, host/UI code, CI, manifests, CSP, docs, registry, and current working tree.

DoD-6 governance publication and owner EXE HITL were not used as failure reasons.

## Requirement and DoD Matrix

| Requirement | Status | Evidence / Gap |
|---|---|---|
| Tauri 2 + Leptos 0.8 | Met | `tauri 2.11.5`; UI lock has Leptos `0.8.20`. |
| Workspace member and UI exclusion | Met | [Cargo.toml](/C:/dev/Dedupe/Cargo.toml:39), [Cargo.toml](/C:/dev/Dedupe/Cargo.toml:43); metadata passed. |
| Overview worker and single-open flow | Met | [matter_cmd.rs](/C:/dev/Dedupe/crates/dedupe-chrome/src/matter_cmd.rs:53): encryption check, one `open_for_read`, `info`, `load_case_overview_on`. |
| Encrypted fail-closed behavior | Met | No `open_*` call follows encrypted detection; test exists. |
| Honest chip mapping | Met | Top-level processed count, privilege/withhold split, custodians `+`, Produced em-dash. |
| Recents/create/open behavior | Met by inspection | MRU/cap/load normalization, injected directories, missing-root retention, parent/name creation, and structured errors are wired. |
| Percent-encoded route identity | Met | Both host/UI helpers avoid double decode; percent regression tests pass directly from the existing host test artifact. |
| Tokens/CSP/a11y | Partial | Plex/paper, CSP, offline fonts, Ctrl+K, and focus-visible exist. Skip-link focus remains defective; see P3. |
| CI and docs | Met by inspection / reported | CI includes wasm target, pinned Trunk, host tests, deny exception, changelog and architecture pointer. Fresh CI gates unavailable locally. |
| DoD-1 | Not independently verifiable | Configuration and metadata are correct; release launch/HITL not observed. Not a failure basis per instruction. |
| DoD-2 | Met by inspection/reported tests | Full tempfile tests blocked locally; prior r2 report recorded 18 passing tests. |
| DoD-3 | Met by inspection/reported tests | Recents and creation tests present; tempfile execution blocked locally. |
| DoD-4 | Partial | Preferred skip targets are present but not focusable. |
| DoD-5 | Reported met, not freshly verifiable | Cargo build lock and advisory DB permissions blocked local gates. |
| DoD-6 | Pending governance | Registry remains In progress; canonical review and ledger publication are pending. Explicitly not the verdict basis. |

## Findings

### [P3] Preferred skip-link landmarks are not focusable

Confidence: High  
Requirement: DoD-4 / §3.4 functional “Skip to matters” and “Skip to counts” links.  
Location: [app.rs](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/app.rs:64), [list.rs](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/list.rs:61), [home.rs](/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/home.rs:70)

Problem: `focus_skip_target` selects the preferred element whenever it exists and calls `.focus()`. However, `#matters` is a plain `<section>` and `#counts` is a plain `<div>`, neither with `tabindex="-1"`. The fallback to `#main-content` therefore does not run, and focusing the preferred target is ineffective.

Failure scenario: On the Matters list, “Skip to matters” leaves focus on the skip link rather than the matters landmark. On Matter Home, “Skip to counts” similarly fails to move focus.

Correction: Make the preferred landmarks programmatically focusable with `tabindex="-1"`, or have the helper validate focusability and fall back to `#main-content`.

Verification: Add/run route-level DOM checks asserting `document.activeElement.id` after activating each skip link on list, home, and stub routes.

Deferrable: No; this is a small bounded fix.

## Completeness Sweep

- Intentional Process/Review/Produce/Admin stubs match scope.
- No production `unwrap()` or `expect()` found in host/UI runtime paths.
- Test-only `expect()` calls and the build-script `panic!` are non-production.
- No daemon, PST pipeline, forbidden dependency, zpdf, coral accent, Google Fonts CDN, or client PST was found.
- Generated `gen`, `dist`, and `target` contents are ignored.
- No sensitive untracked files appear in the working-tree status.

## Wiring and Regression Review

The overview path is correctly wired:

`MatterHome → Tauri invoke → dedicated thread → encryption check → one read open → info + overview → chips`

The list/create path is correctly wired:

`native folder picker → create/remember → encoded route → ParamsMap root → overview`

Prior findings:

- P2-1 through P2-5: closed in source and tests.
- r2 percent-route P2: closed; the four host path tests pass directly.
- r2 skip-link P3: only partially closed. The fallback landmark exists on every route, but preferred targets prevent fallback and cannot receive focus.

## Verification Evidence

Observed:

- `cargo fmt --all --check` — passed.
- `git diff --check` — passed.
- `cargo metadata --no-deps --format-version 1 --locked` — passed.
- `cargo tree -p dedupe-chrome --depth 1` — Tauri `2.11.5`.
- UI lock — Leptos `0.8.20`, Leptos Router `0.8.15`.
- Host artifact listed 18 tests.
- Direct host path regression tests — 4 passed.
- Direct metadata/CSP tests — passed.
- Full host artifact run — 10 passed, 8 tempfile tests blocked by `PermissionDenied` under `%TEMP%`.

Unavailable:

- Fresh Cargo clippy/test/check commands: `C:\dev\Dedupe\target\debug\.cargo-lock` access denied.
- `ledgerful` status/impact/verify: Ledgerful database could not be opened.
- `cargo deny` and `cargo audit`: advisory DB lock unavailable in read-only environment.
- Fresh Trunk build and release EXE launch were not rerun; prior r2 notes report Trunk success.

## Deferred Candidates

None. The remaining P3 is easy and does not qualify for deferral.

## Completion Decision

The implementation is substantially complete and the prior P2 fixes are closed. Fix the preferred skip-link focusability issue, rerun the UI/host gates, and request another completion audit.