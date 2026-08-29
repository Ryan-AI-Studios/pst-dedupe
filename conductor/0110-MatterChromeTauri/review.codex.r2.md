# Track Completion Audit — 0110-MatterChromeTauri

## Verdict: FAIL

## Scope Reviewed

Read-only fresh audit of the working tree on `track/0110-matter-chrome-tauri`, including `spec.md`, `plan.md`, P2 fix notes, host/UI code, CI/configuration, docs, untracked files, and relevant `matter-core`/Leptos APIs.

DoD-6 governance publication and owner EXE HITL were not used as failure reasons.

## Requirement and DoD Matrix

| Requirement / DoD | Status | Evidence | Gap |
|---|---|---|---|
| Tauri 2 + Leptos 0.8 CSR | Met | Cargo locks, manifests, `tauri.conf.json` | None |
| Workspace member/UI exclusion | Met | `Cargo.toml:39-43`; metadata succeeded | None |
| Host crate boundaries | Met | No forbidden dependencies; correct version/license | None |
| Required routes/stubs | Partial | Routes exist in `ui/src/app.rs` | Percent-containing roots are double-decoded |
| Blocking overview command | Met | Dedicated thread → `open_for_read` → `info` → `load_case_overview_on` | None |
| Encrypted fail-closed behavior | Met | Encryption check precedes all `open_*` calls | None |
| Honest overview chips | Met | Correct totals, privilege/withhold, custodians, Produced em-dash | None |
| Recents MRU/cap/missing retention | Met | Normalization and P2 tests present | Runtime test execution blocked by sandbox temp permissions |
| Create/open flow | Met | Parent/name join, validation, native dialog wiring | None |
| Tokens/CSP | Met | Plex fonts, CSP, offline assets, no coral/Google Fonts | Skip-link targets incomplete |
| Scope locks | Met | Intentional 0111–0113 stubs; no daemon/PST pipeline | None |
| CI/test coverage | Met, reported | CI includes wasm target, Trunk, host tests | Full gates not independently executable here |
| DoD-1 | Not verifiable locally | Configuration is correct; no release launch observed | Owner HITL |
| DoD-2 | Met by inspection/reported tests | Host implementation and tests | Local temp-directory permissions blocked execution |
| DoD-3 | Partial | Persistence/create paths are implemented | Route identity defect affects opening valid recents |
| DoD-4 | Partial | CSS/config mostly satisfy requirements | Broken global skip-link targets |
| DoD-5 | Reported met | P2 note reports green gates and 17 host tests | Local Cargo lock access denied |
| DoD-6 | Pending / not verifiable | Registry, canonical review, ledger publish remain pending | Explicitly excluded from failure basis |

## Findings

### [P2] Matter route IDs are double-decoded for valid percent-containing Windows paths

Confidence: High

Requirement: DoD-2/DoD-3 route identity must preserve the absolute UTF-8 matter root.

Location: [`home.rs`](</C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/home.rs:18>), [`path_id.rs`](</C:/C:/dev/Dedupe/crates/dedupe-chrome/ui/src/path_id.rs:48>)

Problem: Leptos Router 0.8 already decodes route parameters through `decode_uri_component`. `MatterHome` and `matter_home_href_from_param` decode the parameter again.

Evidence:

- Leptos `ParamsMap::insert` calls `Url::unescape`.
- CSR `Url::unescape` calls `decode_uri_component`.
- Application code then calls `decode_matter_id` again.
- Existing tests cover spaces, accents, and backslashes, but not literal `%` sequences.

Failure scenario: A valid root such as `C:\Cases\100%20Done` is encoded as `%2520`, decoded by Leptos to the literal `%20`, then decoded again by the application to a space. The overview command receives the wrong path and returns `not_found`.

Correction: Treat `ParamsMap` values as already decoded; remove the second decode from the home and stub back-navigation path. Add a regression test for a decoded root containing literal `%20`/`%25`.

Verification: Run host path tests, UI build, and a route-level browser smoke using the percent-containing root.

Deferrable: No

### [P3] Global skip links point to absent landmarks on most routes

Confidence: High

Requirement: DoD-4 requires functional “Skip to matters” and “Skip to counts” links.

Location: [`app.rs`](</C:/dev/Dedupe/crates/dedupe-chrome/ui/src/app.rs:72>), [`list.rs`](</C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/list.rs:61>), [`home.rs`](</C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/home.rs:72>)

Problem: The global shell always renders both links, but `#matters` exists only on the list route and `#counts` only on the home route. Both targets are absent on stub routes.

Failure scenario: Activating either skip link on the wrong route does not move focus or navigation to a meaningful landmark.

Correction: Use route-specific skip links or provide stable shell landmarks on every route.

Verification: Check each route’s rendered DOM and keyboard focus behavior.

Deferrable: No; this is a small, bounded fix.

## Completeness Sweep

- Intentional Process/Review/Produce/Admin stubs match scope.
- No production `unwrap()`/`expect()` found in host/UI runtime code.
- Test-only `expect()` calls are present.
- Build-script `panic!` is build-time failure handling, not application runtime.
- No forbidden PST, daemon, `process-runner`, `matter-service`, zpdf, coral, or Google Fonts wiring found.
- Unrelated untracked `fixtures/keep_set_summary.json` was inspected; it is not consumed by this track and is not a client PST.

## Wiring and Regression Review

Overview path is correctly wired:

`MatterHome → Tauri invoke → dedicated thread → metadata check → encrypted detection → one read open → info + overview → chips`

List/create path is correctly wired:

`native folder picker → create/open → recents remember → encoded route`

The remaining core regression is route identity preservation for literal percent sequences. The accessibility defect is separate and non-blocking in severity but still violates the stated skip-link requirement.

## Verification Evidence

### Observed now

- `git status --short --branch`
- `git diff --check`
- `cargo fmt --all --check` — passed
- `cargo metadata --no-deps --format-version 1 --locked` — passed; host member present and UI excluded
- Tauri configuration parsed; title, dimensions, CSP, IPC, and `withGlobalTauri` match
- `trunk --version` — 0.21.14
- `cargo tauri --version` — 2.11.4
- `rustc --version` — 1.95.0
- Existing host test artifact listed 17 tests; direct execution observed 9 pure tests passing and 8 tempfile tests blocked by sandbox `PermissionDenied`

### Reported by orchestrator

- `cargo test -p dedupe-chrome` — 17 passed
- `cargo clippy -p dedupe-chrome --all-targets -- -D warnings` — passed
- `trunk build --release` — succeeded
- Workspace tests previously passed

### Not verifiable here

- Fresh Cargo test/clippy/workspace/check runs: Cargo could not open `C:\dev\Dedupe\target\debug\.cargo-lock` due read-only permissions.
- Fresh Trunk build: not rerun because it writes build artifacts.
- Release EXE launch/owner HITL.
- Current Ledgerful status/impact/verification; Ledgerful database access failed with `unable to open database file`.

## Deferred Candidates

None. The P2 route defect is non-deferrable, and the P3 skip-link issue is easy to fix.

## Completion Decision

The P2 fixes appear closed, and the main chrome/overview implementation is substantially complete. However, the fresh audit found a valid route identity regression for matter roots containing percent sequences, plus broken skip-link targets. Fix both, rerun the host/UI gates, and request re-audit.