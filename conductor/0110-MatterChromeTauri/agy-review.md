# Track review: 0110-MatterChromeTauri

**Harness:** Antigravity (`agy`)  
**Track:** `conductor/0110-MatterChromeTauri`  
**Date:** 2026-08-29  

---

## Summary

Line-by-line verification of every Origin claim in `spec.md` against live code on `main` @ `1272ff0`:

1. **Origin Claim 1 (Matter-core schema version & overview APIs):**
   - *Verification:* Verified live in [`crates/matter-core/src/schema.rs:L11`](file:///C:/dev/Dedupe/crates/matter-core/src/schema.rs#L11) (`pub const SCHEMA_VERSION: u32 = 39;`) and [`crates/matter-core/src/overview.rs:L1-175`](file:///C:/dev/Dedupe/crates/matter-core/src/overview.rs#L1-L175).
   - `load_case_overview(root, &OverviewOptions)` fans out read-only rollups across WAL connections. `OverviewTotals` contains `items_total`, `size_bytes_top_level`, `sources_total`, `top_level_items`, and `families_total`. `ReviewOverview` and `PrivilegeOverview` match the exact fields specified.

2. **Origin Claim 2 (Matter creation and encrypted detection):**
   - *Verification:* Verified live in [`crates/matter-core/src/matter.rs`](file:///C:/dev/Dedupe/crates/matter-core/src/matter.rs).
   - `Matter::create(root, name)` initializes unencrypted matters with id `mat_...` and rejects if `matter.db` exists.
   - `is_encrypted_matter(root)` accurately identifies encrypted matters without opening or attempting passphrase unlock.

3. **Origin Claim 3 (Desk Overview parity & workspace configuration):**
   - *Verification:* Verified live in [`Cargo.toml`](file:///C:/dev/Dedupe/Cargo.toml) and [`deny.toml`](file:///C:/dev/Dedupe/deny.toml).
   - `dedupe-desk` is member 27. Workspace currently does not include a Tauri member. `deny.toml` allows `OFL-1.1` fonts and requires explicit `LicenseRef-Proprietary` exceptions for workspace crates.

---

## Blind-Spot Headlines

1. **Host-Target Workspace Test Collision:** If `crates/dedupe-chrome/ui` is not explicitly excluded in root `Cargo.toml` (`exclude = ["crates/dedupe-chrome/ui"]`), `cargo test --workspace` will attempt to compile Leptos CSR for the MSVC host target and fail on missing WASM web-sys symbols.
2. **False-Pass Hazard on Empty Matter Overview Tests:** Testing `matter_overview` only against an empty matter allows hardcoded 0 implementations to pass by accident. Tests must verify that adding sources/items dynamically increments `Sources` and `Processed`.
3. **Shared Recents File Test Flakiness:** If host unit tests write to the real `%LOCALAPPDATA%` recents path, concurrent test execution will race on `recents.json`. The recents store must accept an injectable directory in tests.
4. **WASM Global Tauri Bridge Requirement:** In Tauri 2, Leptos CSR cannot invoke host commands via `window.__TAURI__.core.invoke` unless `app.withGlobalTauri = true` is explicitly configured in `tauri.conf.json`.

---

## Findings (B/M/m/O)

| ID | Sev | Finding with concrete failure scenario | Fix |
|---|---|---|---|
| **F-0110-1** | **Major** | **WASM crate compilation in host workspace test:** If `crates/dedupe-chrome/ui` is discovered as a workspace member, `cargo test --workspace` on MSVC fails. | Add `exclude = ["crates/dedupe-chrome/ui"]` in root `Cargo.toml`. |
| **F-0110-2** | **Major** | **`cargo deny` failure on new proprietary crate:** Adding `crates/dedupe-chrome` without updating `deny.toml` will fail `cargo deny check` in CI. | Add `{ allow = ["LicenseRef-Proprietary"], crate = "dedupe-chrome" }` to `exceptions` in `deny.toml`. |
| **F-0110-3** | **Minor** | **Parallel test collision on `recents.json`:** If `recent_matters_remember` hardcodes the user `%LOCALAPPDATA%` path, parallel test runners will corrupt each other's assertions. | Allow `recent_matters_remember_in(dir, ...)` and use `tempfile::tempdir()` in unit tests. |
| **F-0110-4** | **Minor** | **False-pass hazard on empty overview:** Asserting 0 on an empty matter does not test SQL aggregation correctness. | Add an integration test that creates a matter, inserts a source, and asserts `sources == 1`. |
| **F-0110-5** | **Observational** | **No `expect` in `main.rs` entry point:** Tauri templates often include `.expect("error while running tauri application")`, which violates repo rules. | Return `Result<(), Box<dyn std::error::Error>>` from `main()` and propagate errors. |

---

## What Looks Solid

- **Architectural Scoping:** Clean boundary between Tauri host and Leptos UI; no process runner or PST writer code is pulled into the chrome.
- **Data Honesty:** Overview strictly surfaces 0038 case overview data without inventing fake `Produced = 0` counts or secondary SQL schemas.
- **Offline & Token Rigor:** Self-hosted IBM Plex fonts with OFL-1.1 license compliance; complete avoidance of external CDNs.
- **Fail-Closed Security:** Strict CSP in `tauri.conf.json`, encrypted matters rejected with structured error instead of attempting unverified decryption.

---

## Deferred Fold-In Table

| Deferred ID | Action | Rationale |
|---|---|---|
| `D-0110-matter-chrome` | **Absorb and close** | Fully implemented by Track 0110. |
| `D-0032-01` / `D-0034-02` | **Decline (keep open)** | Native PDF/zpdf rasterizer; owned by Track 0114. |
| `D-0040-01` / `D-0060-04` | **Decline (keep open)** | TIFF G4 / OPT production; owned by Track 0115 (parked). |
| `D-0111` – `D-0116` | **Decline (keep open)** | Proposed tracks for review queue, 3-pane viewer, produce wizard, and process folding. |
| `D-0062-codesign` | **Decline (keep open)** | Release Authenticode signing operations. |

---

## PR / Review Comments the Plan Missed

- None. PRs #107 through #110 contain no outstanding review or Bugbot comments relevant to this surface.

---

## Research / Tools Notes

- **ai-brains:** Used from `C:\dev\Dedupe`. Preflight verified (3886 pinned memories). Decision record `5f7d3835` confirmed for Track 0110.
- **ledgerful:** Used from `C:\dev\Dedupe`. Verified status `0 pending / 0 unaudited drift`.
- **gh cli:** Verified last merged PRs (#110, #109, #108, #107).

---

## Verdict: Ready after fixes

The plan is architecturally sound and adheres to repository invariants. Ensure `Cargo.toml` excludes `ui/`, `deny.toml` includes the crate exception, and tests avoid shared app-data paths.

To fold in these review findings, run:
```powershell
/foldin 0110
```
