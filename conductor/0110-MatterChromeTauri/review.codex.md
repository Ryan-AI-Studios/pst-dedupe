I could not write the file because no `-o` path was supplied and the workspace is read-only. Per the review convention, the intended path is `C:\dev\Dedupe\conductor\0110-MatterChromeTauri\review.codex.md`. Full audit:

# Track Completion Audit — 0110-MatterChromeTauri

## Verdict: FAIL

## Scope Reviewed

Reviewed `spec.md`, `plan.md`, all new `crates/dedupe-chrome` host/UI files, workspace/CI/config/docs changes, internal review notes, and relevant `matter-core` APIs.

Scope is the uncommitted working tree on `track/0110-matter-chrome-tauri`.

## Requirement and DoD Matrix

| Requirement | Status | Evidence | Tests | Gap |
|---|---|---|---|---|
| Tauri 2 + Leptos 0.8 CSR | Met | `Cargo.lock`, UI lock, `tauri.conf.json` | `cargo tree` observed | None |
| Workspace member and UI exclusion | Met | `Cargo.toml:39-43` | `cargo metadata` observed | None |
| Host crate/version/license/boundaries | Met | `crates/dedupe-chrome/Cargo.toml` | Host package resolves | None |
| Required routes and four workspace tabs | Met | `ui/src/app.rs:87-97`, `home.rs:140-145` | Trunk build reported | UI tests not executed |
| Single overview command and blocking worker | Met | `src/lib.rs:27-34`, `matter_cmd.rs:40-67` | Host tests present | Metadata error classification gap below |
| Encrypted fail-closed behavior | Met | `matter_cmd.rs:41-46` | `encrypted_returns_kind_without_open` | No runtime instrumentation, code ordering is correct |
| Honest chip mappings | Met | `matter_cmd.rs:58-66`, `home.rs:78-113` | Source/empty tests present | None |
| Recents MRU, missing-root retention, cap | Partial | `recents.rs:32-69` | One MRU test | Loader bypasses cap; UI drops persistence errors |
| Parent/name matter creation | Met | `create.rs:9-13` | Creation and invalid-name tests | None |
| Native folder picker | Met | `ui/src/pages/list.rs:184-210` | Trunk build reported | No browser/HITL evidence |
| Plex/paper tokens, fonts, CSP | Met | `tokens.css`, `app.css`, `tauri.conf.json:24-32` | CSP host test; static inspection | None |
| Process/Review/Produce/Admin scope locks | Met | Stub files and locked copy | Trunk build reported | None |
| CI wasm target and trunk build | Met | `.github/workflows/ci.yml:92-111` | Reported by implementer | Not independently observed |
| DoD-1 | Not verifiable | EXE configuration is correct | Launch not observed | Owner HITL external |
| DoD-2 | Met | Overview implementation and tests | 5 relevant host tests present | Execution reported, not observed here |
| DoD-3 | Partial | Recents/create/list implementation | Host tests present | Existing-file cap and silent write failure |
| DoD-4 | Met | CSS/config/static assertions | CSP test and static inspection | None |
| DoD-5 | Partial | CI workflow and 10 host tests | Reported gates; formatting observed | UI helper tests not in CI |
| DoD-6 | Partial / pending governance | Changelog and architecture updated | Registry/ledger/review publish pending | Not treated as failure by itself per instruction |

## Findings

### [P2] Existing recents files can exceed the required 20-entry cap

Confidence: High  
Requirement: DoD-3 requires recents to be capped at 20 entries.  
Location: `crates/dedupe-chrome/src/recents.rs:32-38`

Problem: `recent_matters_remember_in` truncates the list, but `recent_matters_list_in` returns persisted entries without enforcing `MAX_RECENTS`.

Evidence: `recent_matters_list_in` returns `file.matters` directly at line 38.

Failure scenario: A pre-existing, manually edited, legacy, or concurrently corrupted `recents.json` contains more than 20 entries. `/matters` renders all entries, violating the cap.

Correction: Normalize loaded recents to `MAX_RECENTS` before returning them, preserving MRU order and evicting the tail.

Verification: Seed an injected `recents.json` with 25 entries, call `recent_matters_list_in`, and assert exactly 20 entries with the tail removed.

Deferrable: No

### [P2] `matter_overview` misclassifies non-missing filesystem errors as `not_found`

Confidence: High  
Requirement: Missing roots must return `not_found`; other failures must return `failed`.  
Location: `crates/dedupe-chrome/src/matter_cmd.rs:28-36`

Problem: `Path::exists()` returns `false` for metadata errors such as access denial, not only for missing paths.

Evidence: Line 30 maps every `!path.exists()` result to `CommandError::not_found`.

Failure scenario: An inaccessible or otherwise unstatable matter root is reported as missing, preventing the UI from distinguishing a bad path from an authorization/filesystem failure.

Correction: Use `fs::metadata` and map only `ErrorKind::NotFound` to `not_found`; map all other metadata errors to `failed`.

Verification: Add an inaccessible-path or equivalent metadata-error test on Windows and assert `kind == "failed"`.

Deferrable: No

### [P2] Recents persistence failures are silently ignored by the production UI

Confidence: High  
Requirement: Recents must be wired end to end without silent failure.  
Location: `crates/dedupe-chrome/ui/src/pages/list.rs:32-44`

Problem: `go_matter` discards the result of `recent_matters_remember`.

Evidence: Line 34 starts the command and line 41 ignores its result with `let _ =`.

Failure scenario: If app-data cannot be created or `recents.json` cannot be written, the user is navigated to the matter with no error. The matter is not recorded in recents and the failure is invisible.

Correction: Handle the error by updating the visible error signal; update the recents signal on success. Decide explicitly whether navigation should proceed after persistence failure.

Verification: Inject a failing recents directory or make the file unwritable and assert the UI surfaces the failure.

Deferrable: No

### [P2] Required route-encoding tests are not part of an executed test gate

Confidence: High  
Requirement: Phase 2 requires an encode/decode round-trip test for Windows paths with spaces, Unicode, and `C:\...`; tests must prove required behavior.  
Location: `crates/dedupe-chrome/ui/src/path_id.rs:52-91`; `.github/workflows/ci.yml:107-111`

Problem: The route tests exist only in the workspace-excluded UI binary. CI runs `trunk build`, which compiles the UI but does not execute `#[cfg(test)]` tests. CI then runs only host tests for `dedupe-chrome`.

Evidence: CI has no `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` or equivalent UI helper-test step.

Failure scenario: A regression in percent-encoded Windows route identity can pass every stated CI gate while breaking navigation to real matter roots.

Correction: Add an executable host-test target for the pure path helper or add a supported UI test command to CI.

Verification: Run the route test suite in CI and assert the Unicode/space/backslash/drive-colon round trips.

Deferrable: No

### [P2] Production recents silently fall back to the system temp directory

Confidence: High  
Requirement: Production recents belong under application data; no silent fallbacks are permitted.  
Location: `crates/dedupe-chrome/src/recents.rs:26-30`

Problem: `production_recents_dir` falls back from `dirs::data_local_dir()` to `std::env::temp_dir()`.

Evidence: Line 28 uses `.unwrap_or_else(std::env::temp_dir)`.

Failure scenario: If the application-data directory cannot be resolved, recents are written to a temporary location. They may disappear during cleanup and path-bearing matter metadata is stored outside the intended app-data location.

Correction: Return a structured failure when app-data resolution fails, or use a Tauri app-data directory API that returns an error.

Verification: Exercise the unavailable-app-data path and assert a visible `failed` error instead of temp-directory persistence.

Deferrable: No

## Completeness Sweep

No production `unwrap`/`expect` was found in the host runtime. Test-only `expect` calls are present. The build script contains a build-time `panic!`, not a runtime path.

No product `#ec3013`, Google Fonts CDN, daemon, `process-runner`, PST reader/writer, `matter-service`, zpdf, or unique-PST dependency was found.

The intentional Process/Review/Produce/Admin stubs match the locked scope copy.

## Wiring and Regression Review

The main overview path is wired:

`MatterHome → Tauri invoke → std::thread → is_encrypted_matter → one open_for_read → info + load_case_overview_on → response → chips`

The create/list path is wired:

`native dialog → create_matter or recent_matters_remember → encoded matter route`

The main wiring defect is that recent persistence errors are discarded. The persistence implementation also does not normalize externally loaded lists to the required cap.

The existing `dedupe-desk` dependency boundary remains intact, and no second processing pipeline or PST access was introduced.

## Verification Evidence

### Observed now

- `git status --short --branch`
- `git diff --check`
- `cargo fmt --all -- --check` — passed
- `cargo metadata --no-deps --format-version 1` — host crate present; UI excluded
- `cargo tree -p dedupe-chrome --depth 1` — Tauri 2.11.5 resolved
- `trunk --version` — 0.21.14
- `cargo tauri --version` — 2.11.4
- Tauri configuration parsed successfully; CSP, window size, title, and identifier match the specification
- Host source contains exactly 10 tests

### Reported by implementer

- `cargo clippy --workspace --all-targets -- -D warnings` — reported OK
- `cargo test -p dedupe-chrome` — reported 10 passed
- `cargo check -p dedupe-desk` — reported OK
- `trunk build` — reported OK
- `cargo test --workspace` — reported OK/previously OK

### Not verifiable

- `cargo test -p dedupe-chrome` could not be rerun because Cargo was denied access to `target\debug\.cargo-lock` in this read-only environment.
- UI test execution could not be rerun for the same reason.
- Ledgerful status could not open its database in the read-only environment.
- Release EXE launch and owner synthetic-matter HITL were not observed.
- Registry completion, canonical `review.md`, and ledger commit remain orchestrator/governance work and are not treated as the reason for this verdict.

## Deferred Candidates

None. All identified findings are P2 and cannot be deferred under the track rules.

## Completion Decision

The core chrome, overview honesty, encrypted fail-closed behavior, CSP, tokens, routes, and scope boundaries are implemented correctly.

Completion is blocked by the five P2 issues above, especially recents cap normalization, incorrect filesystem error classification, and silently discarded persistence failures. DoD-6 governance publication is separately pending and is not the basis for failure.