# 0110 — Matter chrome (Tauri 2 + Leptos)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export (0108–0109),
> queue (**0111**), review window (**0112**), produce (**0113**), zpdf (**0114**),
> OPT (**0115** parked), or Process fold (**0116**). Do not vendor
> `C:\dev\dedupe-frontend`.

- **Track ID:** 0110-MatterChromeTauri
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes `E-Discovery — ideal frontend` + `E-Discovery — recommended stack`. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-29); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (layout, not tokens).
- **Status:** Completed — PR **#111** / `5a76f0b`
- **Depends on:** Series S **0108–0109 Completed** · Desk **0020** (egui Process stays) · `matter-core` schema **v39** + `load_case_overview` (0038)
- **Spec authored:** 2026-08-29 (placeholder → Ready)
- **Series:** O (Review chrome) — first track
>
> **Closes / absorbs:** `D-0110-matter-chrome` (this track). Does **not** close D-0032-01 / D-0034-02 / D-0040-01.
> **HITL:** owner launches the EXE once against a **synthetic** matter (temp or local empty folder). INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign** (not this track).
>
> **Last-PR fold-in (2026-08-29):** PRs **#110, #109, #108, #107** — no Cursor/Bugbot/review comments. Disposition in §2.8.
>
> **Review fold-in (2026-08-29):** `opencode-review.md` + `agy-review.md`. Disposition in §2.10 and `foldin-note.md`. Locks: CSP includes `'wasm-unsafe-eval'` + IPC `connect-src` (no Google Fonts); `matter_overview` = `is_encrypted_matter` then one `open_for_read` + `info` + `load_case_overview_on`; `create_matter` root is `parent.join(validated_name)`; recents MRU/20-cap/injectable dir; `insert_source` is a pub API.
>
> **Stack lock:** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust (CI `dtolnay/rust-toolchain@stable`). Plex / paper / cool chrome. Red = privilege / withhold / blocker only. **One** `matter-core` data command: `matter_overview`. No daemon. No second pipeline.

---

## 1. Objective

Ship a single-process Windows EXE that is the **0020 analog in new chrome**: matter list, matter home with honest 0038 overview chips, and four workspace tabs (Process / Review / Produce / Admin). Process jobs stay in `dedupe-desk` until **0116**. Review/Produce/Admin are labeled stubs that route toward **0111–0113**.

This advances **product correctness** by putting counsel-facing chrome on the **same** `matter-core` SQLite + `load_case_overview` numbers Desk already shows — not a second SQL, not mock coral pixels, not a unique-pst rewrite.

## 2. Context (read before starting)

### 2.1 Why this track, now

Unique-export Series S is Completed (0108 PR #106, 0109 PR #109 / `dc7c29c`). Structural INC* HITL is green. Counsel unique-pst will not get more useful from another honesty ID. The remaining product gap is Relativity-class **chrome** (Hermes IA). Stack constitution is Tauri 2 + Leptos; egui stays Process-only until **0116**.

### 2.2 Live APIs (plan-time 2026-08-29, HEAD `1272ff0`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` / tests | `SCHEMA_VERSION == 39` |
| `overview.rs` | `load_case_overview(root, &OverviewOptions)` fans out `open_for_read` on `std::thread` (never UI thread). `load_case_overview_on` sequential (tests). |
| `OverviewTotals` | `items_total`, `size_bytes_top_level`, `sources_total`, `top_level_items`, `families_total` |
| `ReviewOverview` | `in_review`, `reviewed_count`, `unreviewed_count` |
| `PrivilegeOverview` | `claimed` (active rows), `withhold` (union of flag + table) |
| `ErrorOverview.total` | matter-scoped `item_errors` |
| `by_custodian` + `other_custodians_count` | top-N **labels**; remainder is **item count**, not extra custodians |
| `Matter::create(root, name)` | unencrypted; id `mat_…`; refuses if `matter.db` exists |
| `Matter::open_for_read` | WAL reader; **no** `workspace/temp` wipe. Encrypted needs passphrase / `PST_DEDUPE_MATTER_PASSPHRASE` |
| `is_encrypted_matter` | detect without opening |
| `MatterInfo` | `id`, `name`, `created_at`, `schema_version`, `storage_root` |
| `insert_source` | **pub** source-registration API (`matter.rs`; not `#[cfg(test)]`). Host tests call it via the `matter-core` dep — no test-gate. |
| Desk analog | `dedupe-desk`: `create_matter` + `load_case_overview` on worker (`matter_ui.rs`). Nav is Home/Workspace/Review/Produce — **not** four IA tabs. |
| Workspace `Cargo.toml` | no Tauri member yet. `dedupe-desk` 0.2.0-rc.1, LicenseRef-Proprietary. |
| CI | `cargo fmt` / `clippy --workspace` / `test --workspace` / audit / deny on `windows-latest`, **stable**. No wasm target today. |
| `deny.toml` | OFL-1.1 already allowed (fonts). New crate needs a LicenseRef-Proprietary **exception** row. |
| MS-PST | **N/A this track.** |

### 2.3 Mock (research only; re-verified 2026-08-29)

`C:\dev\dedupe-frontend`: Tauri 2 + Leptos 0.8 CSR + Trunk; window 1440×900; routes `/` = `/review` = 25-row queue; `/process` `/produce`; **no** `/matters`; `src-tauri/src/lib.rs` empty `run()` with `.expect`; tokens Archivo / coral `#ec3013`; CSP `null`.

**Steal:** density, 1440×900, four-tab chrome idea, produce-checklist **layout** (0113). **Do not copy:** coral tokens, ⌘K, empty `expect` run, `dedupe-review` package name, 13-column lead/QC queue as home.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2.11.5** (crates.io stable) | `tauri = "2"`. Reject any **3.x / pre-release** resolve of `tauri` (no 3.x stable exists at plan-time). |
| `leptos` / `leptos_router` | **0.8.20** | `leptos = { version = "0.8", features = ["csr"] }` |
| `zpdf` | 0.13.0 | **0114 only.** Do not add this track. |
| Rust | **stable** (CI) | Leptos 0.8 MSRV 1.88. Do **not** switch the workspace to nightly for start-trunk. |

Official Tauri Leptos guide still documents 0.6-era Trunk + `withGlobalTauri: true` + `devUrl` `http://localhost:1420`. Re-verify `create-tauri-app` leptos template at execute. CSR/SSG only — **no** Leptos SSR, **no** Axum.

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3886 pinned).
- `ai-brains sync query` / recall: Series O is next; 0110–0116 Proposed until this expand; Plex/paper; one invoke; 0115 parked; no BCC. Stale “do 0108 first” / “Desk remains egui” superseded by Series S Completed + this Ready spec.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` **0 pending / 0 unaudited drift** before this tx. Doctor: phantom-promote, sig-pin, completion-unreachable — none block planning.
- Ledger tx for this planning pass: `c374a254-a378-41a4-8de7-2b09182ef177`.
- `scan --impact` after spec write (docs/conductor only expected **LOW**).

### 2.6 How this advances the north star

Not UI polish for its own sake: the EXE must display **the same 0038 rollups** as Desk Overview (sources / top-level items / errors / unreviewed / privilege). Lying chips (fake Produced=0, coral “risk” as primary) fail the track. Unique-export surfaces are unchanged.

### 2.8 Last-PR Cursor comments (mandatory)

| PR | Surface | Disposition |
|---|---|---|
| **#110** | docs 0109 Completed registry | none |
| **#109** | 0109 also-eml classify | none |
| **#108** | chore ignore `.agents/` | none |
| **#107** | docs 0108 merge SHA | none |

No new placeholder. Next free ID remains **0117**. No BCC-default track.

### 2.9 Product locks (do not invent at execute)

See §3.

### 2.10 Review fold-in (2026-08-29)

| Id | Disposition |
|---|---|
| opencode-M1 | **Agree — fold** — pin CSP object in §3.6; DoD-4 asserts non-null + `'wasm-unsafe-eval'`. Official Tauri 2 CSP guide (fetched): wasm frontends **must** include `'wasm-unsafe-eval'` in `script-src`; example `connect-src` is `ipc: http://ipc.localhost`. Drop Google Fonts hosts (self-hosted Plex). |
| opencode-m1 | **Agree — fold** — `insert_source` is a public API; §2.2 rephrased. |
| opencode-m2 | **Agree — partial** — `matter_overview` must **not** call `open_*` on encrypted roots (`is_encrypted_matter` first). Encrypted test asserts error kind `encrypted` (not opened / `PassphraseRequired`). If a test touches `PST_DEDUPE_MATTER_PASSPHRASE`, mutex + restore — do **not** add `serial_test` unless needed. |
| opencode-m3 | **Agree — fold** — recents: MRU-front, truncate tail to 20, missing root still listed, `matter_overview` → `not_found`. |
| opencode-m4 | **Agree — fold** — `create_matter` = Desk `parent.join(validated_name)` then `Matter::create(&root, name)`. DoD-3: `<parent>/<name>/matter.db` exists. |
| opencode-o1 | **Agree — fold** — Phase 2 encode/decode helper test (spaces, `é`/`ü`, `C:\…`). |
| opencode-o2 | **Agree — fold** — Phase 0 wording: reject `tauri` 3.x / pre-release (not only “tauri-cef”). |
| opencode-o3 | **Agree — fold** — Phase 0: pin `tauri-cli` `^2` and `trunk` install (dev). CI still may residual `D-0110-ci-trunk`. |
| opencode-o4 | **Agree — fold** — required shape: one `open_for_read`, then `info()` + `load_case_overview_on`. Not fan-out `load_case_overview` (that opens many times). |
| opencode-o5 | **Decline** — pin-count drift is not a product lock. Phase 0 does not re-count vault pins. |
| agy-F-0110-1 | **Already covered** — §3.1 `exclude` `crates/dedupe-chrome/ui`. |
| agy-F-0110-2 | **Already covered** — §3.1 `deny.toml` LicenseRef-Proprietary exception. |
| agy-F-0110-3 | **Agree — fold** — recents store takes an injectable directory; tests use `tempfile`, never live `%LOCALAPPDATA%`. |
| agy-F-0110-4 | **Agree — partial** — DoD-2 already requires Sources=1 after `insert_source`. Also assert **Processed remains 0** (insert_source does not create items — catches hardcoded-zero **and** `items_total` mix-up). Do **not** require a full item insert this track. |
| agy-F-0110-5 | **Already covered** — §3.1/`§3.6` no `expect` in `main`. |
| agy headline `withGlobalTauri` | **Already covered** — §3.6. |

---

## 3. In scope

### 3.1 Crate layout (names are locked)

```
crates/dedupe-chrome/                 # Tauri host — workspace MEMBER
  Cargo.toml                          # package name dedupe-chrome, bin dedupe-chrome
  src/lib.rs                          # commands; no unwrap/expect
  src/main.rs                         # Result-returning entry; no expect("error while running")
  tauri.conf.json
  capabilities/default.json
  icons/                              # can start from mock icons; product name Dedupe Desk
  ui/                                 # Leptos CSR — workspace EXCLUDE
    Cargo.toml                        # package name dedupe-chrome-ui
    index.html
    Trunk.toml                        # port 1420, watch.ignore src-tauri/host
    src/…
    styles/tokens.css                 # Plex/paper (NOT mock coral)
    fonts/                            # self-hosted IBM Plex OFL-1.1 woff2
```

- Workspace `members` += `"crates/dedupe-chrome"`.
- Workspace `exclude` += `"crates/dedupe-chrome/ui"` so `cargo test --workspace` does not try to host-compile wasm.
- `deny.toml` exceptions += `{ allow = ["LicenseRef-Proprietary"], crate = "dedupe-chrome" }` (and `dedupe-chrome-ui` if deny sees it).
- Version `0.2.0-rc.1`. LicenseRef-Proprietary + `../../LICENSE`.
- **Do not** depend on `dedupe-desk`, `process-runner`, `pst-reader`, `pst-writer`, `matter-service`.
- Host depends on `matter-core` (default features — **no** `cloud-s3`), `camino`, `serde`, `thiserror`, `tauri` 2, `tauri-plugin-dialog`.
- Identifier: `com.dedupe.desk.chrome`. Window title: `Dedupe Desk`. Product name: `Dedupe Desk` (not “Dedupe Review”).

### 3.2 Routes

| Route | Screen |
|---|---|
| `/matters` | Matter list: recents cards + New matter + Open… |
| `/matters/:id` | Matter home: chip strip + CTA trio + four tabs |
| `/matters/:id/process` | Stub: “Process stays in Dedupe Desk until 0116.” |
| `/matters/:id/review` | Stub: “First-pass queue is 0111.” |
| `/matters/:id/produce` | Stub: “Produce checklist is 0113.” |
| `/matters/:id/admin` | Inert stub. |

`:id` is the **percent-encoded absolute UTF-8 matter root path** (Desk identity is a folder). Do not invent a global matter registry.

Windows chords: `Ctrl+K` focuses the matter-list search (or no-ops with visible hint if search is empty). **Not** ⌘K. Do not steal Ctrl+F.

### 3.3 Commands (host)

**Exactly one `matter-core` data command:**

`matter_overview { root: String } -> MatterOverviewResponse`

- On a **blocking worker** (Tauri `spawn_blocking` or `std::thread` — never the WebView thread, never a Tokio worker doing SQL):
  1. If the path is missing → `not_found`.
  2. If `is_encrypted_matter(root)` → structured error `encrypted` **without** calling `open` / `open_for_read` / `open_with_passphrase`. No passphrase dialog this track.
  3. Else `Matter::open_for_read` **once**, then `info()` + `load_case_overview_on(&matter, &OverviewOptions::default())`. Do **not** call fan-out `load_case_overview` (that opens extra WAL connections).
- Other errors → `failed` with `to_string()` (no panic).
- Response is counts + `MatterInfo` fields only. **Never** subjects, bodies, paths of mail items, or CAS bytes.

Chip mapping (locked — no new SQL / no schema bump):

| Chip label | Source | Honesty rule |
|---|---|---|
| Sources | `totals.sources_total` | |
| Processed | `totals.top_level_items` | top-level only (not `items_total`, which includes attachments) |
| Exceptions | `errors.total` | |
| Unreviewed | `review.unreviewed_count` | |
| Privileged | `privilege.claimed` | active claims; **not** withhold |
| Withhold | `privilege.withhold` | separate pill; red allowed |
| Custodians | `by_custodian.len()` as u64; suffix `+` if `other_custodians_count > 0` | top-N labels; remainder is **items**, not extra custodians — tooltip must say so |
| Produced | **omit numeric** — show `—` + “0113” | do **not** fake `0` |

Also show `name`, `schema_version`, `generated_at` (RFC3339). Tooltip on Processed: same copy as Desk “top-level / not attachments”.

**Filesystem helpers (allowed; not a second pipeline):**

| Command / plugin | Role |
|---|---|
| `tauri-plugin-dialog` | native folder picker (Open… / New matter parent) |
| `recent_matters_list` / `recent_matters_remember` | JSON; paths + display name only. **MRU:** remember promotes to front; then truncate **tail** to max **20**. Missing-on-disk roots **stay listed**; `matter_overview` on them returns `not_found`. Production path is app-data (e.g. `%LOCALAPPDATA%\com.dedupe.desk.chrome\recents.json`). **Tests inject a directory** (`recent_matters_*_in(dir, …)` + `tempfile`) — never the live LocalAppData file. |
| `create_matter { parent, name }` | Copy Desk (`matter_ui.rs`): validate name, `root = parent.join(validated_name)`, then `Matter::create(&root, name)`. Unencrypted only. Do **not** call `Matter::create(parent, name)` (that would put `matter.db` in the picked parent). |

Recents file: never commit. Never store passphrases.

### 3.4 Tokens / a11y

From ideal-frontend (locked 2026-08-26):

| Token | Value |
|---|---|
| Chrome bg | `#0F1419` |
| Document surface | `#F6F4EF` |
| Coding pane (stubs may unused) | `#E8EEF2` |
| Ink | `#1A1F24` |
| Focus ring | `#0B57D0` 2px |
| Privilege / withhold / blocker | `#9B2C2C` |
| UI sans | IBM Plex Sans 13/18, 600 labels |
| Mono | IBM Plex Mono 12/16 |
| Radius | 4px controls, 2px pills |

Self-host OFL-1.1 woff2 (Plex Sans 400/600 + Plex Mono 400). **No** Google Fonts CDN (offline). `deny.toml` already allows OFL-1.1.

Keep `:focus-visible`. Skip links: “Skip to matters” / “Skip to counts”. Window 1440×900, min 1024×640.

### 3.5 Process / Review / Produce this track

- Process tab does **not** start jobs, pick PSTs, or spawn `dedupe-desk`.
- Continue review → `/matters/:id/review` stub.
- Ingest CTA → Process stub copy.
- Produce CTA → `/matters/:id/produce` stub.
- Admin inert.

`dedupe-desk` must still `cargo check` / stay in workspace tests.

### 3.6 Security / hygiene

- `app.security.csp` is **not** `null`. Pin this object (Tauri 2 CSP guide: wasm frontends need `'wasm-unsafe-eval'`; invoke needs IPC connect-src). **No** Google Fonts / CDN hosts:

```json
"csp": {
  "default-src": "'self'",
  "script-src": "'self' 'wasm-unsafe-eval'",
  "connect-src": "'self' ipc: http://ipc.localhost",
  "font-src": "'self'",
  "style-src": "'self' 'unsafe-inline'",
  "img-src": "'self' data:"
}
```

- Capabilities: dialog + the listed commands. No blanket `fs:default` over the whole disk.
- `withGlobalTauri: true` so wasm can `invoke`.
- Production: no `unwrap` / `expect`. Mock’s `.expect("error while running the dedupe-review application")` is forbidden. `main` returns `Result`.
- Never mutate source PSTs. Never commit client PSTs, `output/`, `evidence/`, or matter folders with mail.

## 4. Out of scope (do NOT do here)

- **0111** virtualized queue / 60k rows / saved-search builder.
- **0112** three-pane review window / coding / keyboard 1/2/3.
- **0113** produce checklist / DAT wizard (stub only).
- **0114** zpdf / pdfium / Image tab raster.
- **0115** TIFF/OPT (parked).
- **0116** folding egui Process / launching Desk from the tab.
- Encrypted create/open/passphrase UX (Desk / 0057). Fail closed with `encrypted`.
- Axum daemon, Leptos SSR, nightly Rust, `tauri` 3.x / pre-release.
- Vendoring mock `tokens.css` / Archivo / coral.
- Schema bump, new overview SQL, Produced count, BCC-default, unique-pst flags.
- Legal hold, TAR, auto-privilege, StoryBuilder, clawback, LFP.
- Authenticode (`D-0062-codesign`).
- MSI/cargo-packager as a release program (debug/release EXE is enough).

## 5. Preconditions & dependencies

- **P1 (blocking):** `matter-core` `SCHEMA_VERSION` 39 + `load_case_overview` still present. Re-verify at execute.
- **P2:** Windows WebView2 on operator machines (Tauri 2). CI `windows-latest` has it.
- **P3:** `wasm32-unknown-unknown` + `trunk` on the implementer machine (and CI chrome job).
- *Verified to date:* §2.2–2.4. Last-PR comments empty.

## 6. Risks

| Risk | Mitigation |
|---|---|
| `cargo test --workspace` compiles leptos for host | `exclude` the `ui/` crate. |
| Two pipelines | No `process-runner`. Overview is read-only `open_for_read`. |
| Coral / mock port | Tokens table §3.4; visual DoD: no `#ec3013` in product CSS. |
| Encrypted matter panic | Detect `is_encrypted_matter` first; structured `encrypted` error. |
| Overview on UI thread | `spawn_blocking` / dedicated thread, same contract as Desk `OverviewLoadState`. |
| `other_custodians_count` misread as extra custodians | Chip tooltip + suffix `+` only. |
| Fake Produced=0 | Em-dash + 0113. |
| CI timeout on full `tauri build` | Host tests are the workspace gate; chrome job is `trunk build` + `cargo test -p dedupe-chrome`, not MSI. |
| License / deny | Proprietary exception; OFL for Plex; Tauri Apache-2.0 OR MIT. |
| Pin drift | Phase 0 re-verifies tauri/leptos stables. |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Crate + EXE:** `crates/dedupe-chrome` is a workspace member; `ui/` excluded. `cargo tauri dev` / release EXE launches a 1440×900 window titled `Dedupe Desk`. Four tabs visible on matter home. `dedupe-desk` still builds. No daemon.
- [ ] **DoD-2 — Overview honesty:** `matter_overview` on a blocking worker: `is_encrypted_matter` first; else one `open_for_read` + `info` + `load_case_overview_on`. Empty matter: Sources=0, Processed=0, Exceptions=0, Unreviewed=0, Privileged=0. After pub `insert_source`: Sources=1 **and** Processed still 0. Encrypted: error kind `encrypted`, no `open_*`, no panic. Produced chip is `—`. Custodians tooltip mentions top-N. Response has no subject/body fields.
- [ ] **DoD-3 — List + create:** `/matters` shows recents (MRU-front, cap 20 tail-evict, missing roots still listed). New matter: `root = parent.join(validated_name)` and `<parent>/<name>/matter.db` exists; invalid name rejected. Open… uses native dialog. Recents tests use an **injected** tempfile dir. `matter_overview` on a missing recents root → `not_found`.
- [ ] **DoD-4 — Tokens + a11y + CSP:** Product CSS uses Plex/paper/`#0B57D0`; **no** `#ec3013` as accent. Skip links + `:focus-visible`. Windows `Ctrl+K`. Self-hosted OFL fonts (offline). `tauri.conf.json` CSP is non-null and `script-src` contains `'wasm-unsafe-eval'`; `connect-src` includes `ipc:` / `http://ipc.localhost`; no `fonts.googleapis.com` / `fonts.gstatic.com`.
- [ ] **DoD-5 — Tests + CI:** Host unit tests cover DoD-2 (tempfile `Matter::create`, no client PST). `cargo test -p dedupe-chrome` is in workspace. CI: existing jobs stay green; add wasm target + `trunk build` for `ui/` **or** record `D-0110-ci-trunk` if infrastructure blocks (host tests still required). `cargo deny` exception present. No production `unwrap`/`expect`.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; short `ARCHITECTURE.md` pointer; `D-0110-matter-chrome` closed; ledger committed (`FEATURE`). Unblocks **0111** / **0113**.

**Owner HITL (not CI):** launch EXE, create/open a synthetic matter, confirm chips match Desk Overview on the same root (empty is enough). INC* waived.

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p dedupe-chrome
cargo check -p dedupe-desk
# after ui crate exists:
# rustup target add wasm32-unknown-unknown
# trunk build --config crates/dedupe-chrome/ui/Trunk.toml --release
```

## 9. Deferred (absorb / decline)

| Row | Disposition |
|---|---|
| **D-0110-matter-chrome** | **Absorb / close** on Implement. |
| **D-0032-01** / **D-0034-02** | Remain; owner **0114**. |
| **D-0040-01** / **D-0060-04** | Remain parked; owner **0115**. |
| **D-0111 / D-0112 / D-0113 / D-0116** | Remain Proposed. Stubs only here. |
| **D-0108-keepset-crc-retaint** | Unique-export. **Decline.** |
| **D-0067-embedded-depth** | Matter children. **Decline.** |
| **D-0062-codesign** | Release ops. **Decline.** |
| **D-0063-05** | egui passphrase widgets. **Decline** (no passphrase UX here). |
| **D-0020-01** | egui click-path smoke. Analog HITL is owner-local; not this row. |
| Mock `tokens.css` retune | `C:\dev\dedupe-frontend` only. Not a Dedupe ID. |
| Produced numeric chip | **0113**. Em-dash here. |
| Encrypted open | Desk. Structured error only. |
| Distinct custodian SQL | **Decline** this track (top-N + `+`). Optional later polish — do **not** mint 0117 for it. |
| Last-PR comments #110–#107 | None. |
| opencode-M1 CSP | **Folded** — §3.6 + DoD-4. |
| opencode-m2 env isolation | **Folded partial** — never `open_*` on encrypted; assert kind `encrypted`. |
| agy-F-0110-3 recents inject | **Folded** — tempfile dir in tests. |
| opencode-o5 pin-count | **Declined** — not a product lock. |

**Do not close** D-0032 / D-0034 / D-0040 in 0110.

If `trunk` cannot be installed in CI, mint **no new ID**: add `D-0110-ci-trunk` (P3) in `docs/deferred.md` and keep host tests as the gate.

---

## Series O index (do not reorder)

| ID | Item | After this plan |
|---|---|---|
| **0110** | Matter chrome + one overview command | **Completed** (PR **#111** / `5a76f0b`) |
| **0111** | Virtualized first-pass queue | Proposed |
| **0112** | Three-pane review window | Proposed |
| **0113** | Produce checklist; DAT only | Proposed |
| **0114** | zpdf raster + geometric redact | Proposed |
| **0115** | TIFF G4 + OPT | **Parked** |
| **0116** | Fold egui Process | Proposed |

Next free conductor ID: **0117**.
