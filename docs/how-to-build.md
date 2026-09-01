# How to build (Windows)

**Audience:** engineering. Counsel handoff still needs Authenticode (`docs/release-signing.md`; residual **D-0062-codesign**).  
**Product:** **0.2.0-rc.1**, schema **41**.  
**CWD:** always `C:\dev\Dedupe` unless a command says otherwise. PowerShell: no `&&`.

This page is the lookup for “which command makes which EXE.” Day-1 operator runbook stays [`docs/operator-golden-path.md`](operator-golden-path.md).

---

## What a chrome rebuild includes

`cargo tauri build` for **dedupe-chrome** rebuilds:

| Layer | What |
|---|---|
| Frontend | `crates/dedupe-chrome/ui` → WASM + `ui/dist` (Trunk release) |
| Host | `crates/dedupe-chrome` + its workspace deps, **release** profile |

It does **not** rebuild `pst-dedup.exe`, `dedupe-desk.exe`, or `pst-dedup-gui.exe`. Those are separate `cargo build --release -p …` (or `scripts/package-release.ps1`).

Example from 2026-08-31: chrome EXE written at 18:40; CLI on disk was still 2026-08-29. Ingest/extract HITL used the older CLI; review-window HITL used the new chrome EXE.

---

## Chrome (Tauri 2 + Leptos) — review / Process / Produce UI

This is the EXE for track **0118** HITL (`target\release\dedupe-chrome.exe`).

### Prereqs

```powershell
rustup target add wasm32-unknown-unknown
cargo install trunk --locked --version 0.21.14
# cargo-tauri 2.x (`cargo tauri --version`)
```

### Pitfalls (do not skip)

1. **`NO_COLOR=1` breaks Trunk.** Cursor/agent shells often set it. Trunk maps that to `--no-color` and clap wants `true`/`false`, not `1`. Unset before Trunk:

```powershell
Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue
```

2. **`cargo tauri` runs `beforeBuildCommand` from the workspace root**, not `crates/dedupe-chrome`. So `trunk build --config ui/Trunk.toml` (as in `tauri.conf.json`) fails with “neither a file nor a directory”. Build Trunk yourself from the ui crate, then skip the frontend step.

3. **Do not `--ci` on Trunk-backed builds** in this environment — same `--no-color` failure.

4. **Unsigned.** `--no-sign` is expected locally. Do not ship this EXE as the operator ZIP.

### Commands (copy/paste)

```powershell
Set-Location C:\dev\Dedupe
Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue

Set-Location C:\dev\Dedupe\crates\dedupe-chrome\ui
trunk build --release --config Trunk.toml

# If Trunk’s nested cargo dies with exit 0xffffffff, compile WASM first, then re-run Trunk:
# cargo build --release --target wasm32-unknown-unknown --manifest-path C:\dev\Dedupe\crates\dedupe-chrome\ui\Cargo.toml
# trunk build --release --config Trunk.toml

Set-Location C:\dev\Dedupe\crates\dedupe-chrome
# Skip beforeBuildCommand: Trunk already wrote ui/dist.
$skip = Join-Path $PWD "tauri.build-skip-frontend.json"
@'
{ "build": { "beforeBuildCommand": "" } }
'@ | Set-Content -Path $skip -Encoding utf8
cargo tauri build --no-bundle --no-sign -c $skip
Remove-Item -Force $skip
```

**Output:** `C:\dev\Dedupe\target\release\dedupe-chrome.exe` (product name “Dedupe Desk” in the window title).

NSIS/MSI: omit `--no-bundle` (slower; still unsigned unless you sign).

### Dev (not HITL)

```powershell
Set-Location C:\dev\Dedupe\crates\dedupe-chrome
Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue
cargo tauri dev
```

`beforeDevCommand` is `trunk serve --config ui/Trunk.toml` — same cwd pitfall. If serve fails, run `trunk serve --config Trunk.toml` from `crates\dedupe-chrome\ui` in another terminal, then `cargo tauri dev`.

---

## Workspace host binaries (CLI / Desk / legacy GUI)

From repo root (ui crate is **workspace-excluded**; these do not compile WASM):

```powershell
Set-Location C:\dev\Dedupe
cargo build --release -p pst-dedup-cli
cargo build --release -p dedupe-desk
cargo build --release -p pst-dedup-gui
```

| Binary | Path |
|---|---|
| Chrome (review window) | `target\release\dedupe-chrome.exe` |
| Desk (egui shell) | `target\release\dedupe-desk.exe` |
| CLI | `target\release\pst-dedup.exe` |
| Legacy scan GUI | `target\release\pst-dedup-gui.exe` |

RC zip (desk + cli + gui + SBOMs, **not** chrome):

```powershell
powershell -File scripts\package-release.ps1
```

---

## Verify (not a substitute for the EXE)

```powershell
Set-Location C:\dev\Dedupe
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --manifest-path crates\dedupe-chrome\ui\Cargo.toml
cargo test -p dedupe-chrome
```

`ledgerful verify` runs the same three workspace steps. The **test** step timeout in `.ledgerful/config.toml` must be ≥ ~400s on a warm machine (300s has timed out). Fixture PST tests need `$env:CARGO_TARGET_DIR = 'C:\dev\Dedupe\target'` if a sandbox `CARGO_TARGET_DIR` hides `fixtures\`.

Chrome UI clippy (`cargo clippy` inside `crates\dedupe-chrome\ui`) is **not** the workspace gate; CI chrome-ui is Trunk + `cargo test -p dedupe-chrome` + ui `Cargo.toml` tests.

---

## HITL notes (0118 review window)

- Use the **unsigned chrome EXE** above, not Desk.
- Preferred: synthetic 3-doc Unreviewed family — rapid Save & Next / `[` `]`, then Enter on the last item, un-check privilege, Save/Enter.
- Source PSTs stay **read-only**. Matter trees belong under `output\` (gitignored). Never commit client PSTs, `Matters\`, or `output\`.
- Unique-pst INC* dumps are **not** this page’s gate.
