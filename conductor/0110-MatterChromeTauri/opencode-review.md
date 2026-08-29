# Track review: 0110-MatterChromeTauri

**Harness:** OpenCode (`opencode`)
**Track:** `conductor/0110-MatterChromeTauri`
**Date:** 2026-08-29
**Mode:** review only — no implement, no fold.

## Summary

This is the first frontend track, so the review did what a unique-pst track never needs:
online crate-pin verification (crates.io API, fetched live) plus the usual line-level
live-code audit. Result: **every §2.2 matter-core / CI / Desk-analog pin verifies exact at
HEAD `1272ff0`**, every §2.3 mock claim verifies against `C:\dev\dedupe-frontend` on disk,
and both crate pins are current **today** (tauri 2.11.5 max stable, no 3.x stable exists;
leptos 0.8.20 max stable; leptos current-line MSRV `rust_version` = 1.88 — spec's "Leptos 0.8
MSRV 1.88" is right). The chip map is semantically honest against the live `CaseOverview`
struct (including the two easy traps: `top_level_items` ≠ `items_total`, and
`other_custodians_count` being an *item* remainder — both pinned correctly in §3.3).

The one real gap: **the CSP sentence in §3.6 will white-screen the release EXE if followed
literally** — official Tauri 2 docs require `'wasm-unsafe-eval'` in `script-src` for any wasm
frontend, and the spec's "Tauri 2 default + `'self'`" wording omits it (dev mode hides this;
it fails exactly at the Phase-4 release/HITL gate, and no DoD asserts CSP at all). That plus
four small pin-oversights is everything I found; no B.

| Pin | Live @ `1272ff0` | Verdict |
|---|---|---|
| `SCHEMA_VERSION == 39` | `matter-core/src/schema.rs:11` | ✅ |
| `load_case_overview(root, opts)` on thread fan-out | `overview.rs:188`; `load_case_overview_on` `:302` (sequential, for tests); thread join `:276`; `OverviewOptions` `:32-41` | ✅ |
| `OverviewTotals` fields | `:65-75` — `items_total`, `size_bytes_top_level`, `sources_total`, `top_level_items`, `families_total` | ✅ all 5 |
| `ReviewOverview` | `:92-97` — `in_review`, `reviewed_count`, `unreviewed_count` | ✅ |
| `PrivilegeOverview.claimed/.withhold` | `:102-107`; withhold = flag **or** table (doc :105-106) | ✅ spec §2.2 "union" exact |
| `ErrorOverview.total` | `:122-124` | ✅ |
| Custodian top-N semantics | `overview_by_custodian` `:419-421` → `group_by_label_top_n` `:664-707`: remainder = sum of *all* group counts − top-N sum (**items**, not custodians); `LIMIT` honored, top_n=0 honored | ✅ §3.3 chip rule is grounded |
| `Matter::create(root, name)` unencrypted, refuse on existing db/header | `matter.rs:1085-1092` (bails `MatterAlreadyExists` if `matter.db` or crypto header), `id = new_id("mat_…")` `:1103` | ✅ |
| `open_for_read` = `open_inner(root, false)` — no `workspace/temp` wipe | `matter.rs:1258-1260`; temp wipe only via `create` `:1127` / explicit cleanup flag `:1397-1403` | ✅ |
| `is_encrypted_matter` detect-without-open | `crypto/header.rs:107` | ✅ |
| `MatterInfo` 5 fields | `matter.rs:118-124` | ✅ |
| Passphrase env | `passphrase_from_env()` read inside `open_inner` `:1405-1406` (`PST_DEDUPE_MATTER_PASSPHRASE`) | ✅ |
| Desk analog | `matter_ui.rs:15-20` `create_matter` = `parent.join(validated_name)` + `Matter::create(&root, name)`; `:313` overview on worker thread, never egui thread | ✅ |
| `validate_matter_name` | `dedupe-desk/src/params.rs:647-656` — exactly the 9 chars §3.3 lists + trim/empty | ✅ |
| Workspace members / no Tauri | `Cargo.toml` has no tauri/leptos; no `crates/dedupe-chrome` on disk; `dedupe-desk` = `0.2.0-rc.1` | ✅ |
| CI | `ci.yml` — all 6 jobs `windows-latest` + `dtolnay/rust-toolchain@stable`; no nightly | ✅ |
| `deny.toml` | OFL-1.1 allowed :51; LicenseRef rows for existing crates (copy pattern for `dedupe-chrome`) | ✅ |
| Desk nav "not four IA tabs" | `nav.rs:5+` `Screen` enum = Home/Workspace/StubReduce/Review/Produce/Gap/People/… | ✅ (actually 8+ screens — claim understated, still true) |
| Mock (research-only) | `dedupe-frontend/src-tauri/tauri.conf.json`: `com.dedupe.review`, CSP **null**, title "Dedupe / Review", 1440×900 / min 1024×640; `lib.rs` has exactly the forbidden `.expect("error while running the dedupe-review application")`; ui crate = leptos 0.8 csr + trunk port 1420 | ✅ §2.3/§3.6 claims all reproduce |
| §2.5 ledger tx | `c374a254` (2026-08-29 17:05, "Plan-track 0110…") in ledger search | ✅ |
| Registry | conductor.md:293 / sequencing.md:125,246-250 / ROADMAP.md:427 — 0110 **Ready**, owns Series O head | ✅ |

**Online research (crates.io API, live):**

| Crate | Spec pin | Live | Verdict |
|---|---|---|---|
| `tauri` | 2.11.5; `tauri = "2"` | max stable **2.11.5** (2026-07-01); no 3.x of any kind in version list | ✅ exact |
| `leptos` | 0.8.20; `0.8` + `csr` | max stable **0.8.20** (2026-06-25); only `0.9.0-beta` beyond | ✅ exact |
| MSRV | Leptos 0.8 → 1.88 | leptos current-line `rust_version` 1.88 (tauri 1.78) | ✅ |
| zpdf | 0.13.0, 0114-only | not added this track | ✅ n/a |

**Tauri 2 docs (v2.tauri.app, CSP page, fetched live):** *"When using Rust to develop your
frontend, or if your frontend otherwise uses WebAssembly, remember to include
`'wasm-unsafe-eval'` as a `script-src`."* — this is the crux of M1.

## Findings (B/M/m/O)

No B. One M, four m, four o.

### M1 — CSP under-pinned: missing `'wasm-unsafe-eval'` (+ IPC connect-src) → silent release-EXE white screen, and no DoD gates CSP

§3.6 says: "`app.security.csp` is **not** `null`. Start from Tauri 2 default + `'self'` for
trunk wasm." Tauri 2 does **not** auto-add `'wasm-unsafe-eval'` — the official CSP guide says
the developer must include it whenever the frontend is wasm, which Leptos CSR always is. A
CSP of the obvious shape (`default-src 'self'` etc.) loads chrome but Leptos never mounts:
**dev via `devUrl` can look fine (CSP is enforced on bundled prod assets), so the failure
surfaces exactly at the Phase-4 owner HITL / release build** — the worst place, and HITL
notes say nothing about diagnosing it. Invoke needs the same treatment: the official example
CSP carries `connect-src: ipc: http://ipc.localhost`. Also note the docs example's
`font-src: https://fonts.gstatic.com` / `style-src … https://fonts.googleapis.com` must be
**dropped** here — this track is self-hosted Plex, offline-only.

Fix (one sentence + one token each): pin the CSP object in §3.6, and add a CSP assertion to
DoD-4 (or DoD-2):

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

and in DoD-4: "`tauri.conf.json` csp is non-null and `script-src` contains
`'wasm-unsafe-eval'`" (host-test can assert the file parses; the visual proof stays HITL).

### m1 — "insert_source exists for tests" is wrong as written — it's a plain pub API

§2.2 last-ish row and §3.3 imply `insert_source` is a test-only fixture helper. Live:
`matter.rs:1985`, `pub fn insert_source(&self, path, kind, status, cursor_json)` — a fully
public source-registration API (the doc comment says "Register a source path"), no
`#[cfg(test)]` anywhere above it. Consequence is *good* news the plan should claim: DoD-2's
"After `insert_source`, Sources=1" unit test can live in `cargo test -p dedupe-chrome` with
the stock dependency — no matter-core changes, no test-gate work. Rephrase the §2.2 row to
"pub API (usable from host tests)" so nobody burns time hunting for a gate.

### m2 — DoD-2 encrypted-path test needs an env-isolation pin (`PST_DEDUPE_MATTER_PASSPHRASE`)

`open_inner` reads `passphrase_from_env()` before deciding how to open (`matter.rs:1405-1406`).
If the implementing machine (or any future CI) exports `PST_DEDUPE_MATTER_PASSPHRASE`, the
DoD-2 "encrypted → structured error, no panic" test silently flips from `encrypted` to
"opened/wrong-passphrase" behavior. Pin in plan Phase 1: the encrypted-path test
`std::env::remove_var("PST_DEDUPE_MATTER_PASSPHRASE")` in a guard, and either serialize the
test (env is process-global and `cargo test` is parallel) or assert only on
"error, and error kind ≠ opened". One line in plan Phase 1; cheap insurance against a
flake that looks like a product bug.

### m3 — Recents: no pinned ordering/eviction/missing-root policy

§3.3 pins "max 20 roots, paths + display name only" but not: (a) ordering — pin
most-recently-used-first with `recent_matters_remember` promoting to front (that's what
"recents" means in Desk analog too); (b) eviction — truncate to 20 *after* promote, drop the
tail, test asserts size cap + order; (c) display when a root no longer exists on disk —
spec is silent; Desk shows the entry with an error on open. One sentence for each (suggest:
LRU order, truncate tail, still shown + `not_found` on invoke) — otherwise DoD-3's "recents
persist (max 20)" has no testable AC for cap/eviction.

### m4 — `create_matter { parent, name }`: pin `root = parent.join(validated_name)`

`Matter::create(root, name)` takes the **matter directory** as root (`matter.rs:1085`; the
doc block at :1074-1082 shows `matter.db` etc. created *inside* root) — §3.3's
`create_matter { parent, name }` must mean Desk's pattern `parent.join(validated_name)`
(`matter_ui.rs:15-19`), with the returned root stored in recents. Unpinned, an implementer
can read the command as `Matter::create(parent, name)` — creating the matter *in* the
chosen parent itself (name only in SQLite). One sentence + a DoD-3 assert that
`<parent>/<name>/matter.db` exists.

### o1 — `:id` route param round-trip needs an encoding helper test

§3.2 defines `:id` as the percent-encoded absolute UTF-8 matter root (Windows backslashes,
drive colon, Unicode — the hardest possible route param). Leptos 0.8 router decodes params;
backslash/colon survive only if encoded. Suggest a Phase-2 helper + unit test:
encode → route segment → decode → invoke `matter_overview` equals original root, for a path
with spaces, `é`/`ü`, and `C:\…`. Keeps Desk's folder-identity contract from regressing on a
UTF-8 path edge.

### o2 — "Do not take tauri-cef 3.x alpha" names the wrong thing

crates.io `tauri` has no 3.x at all (verified); the thing the spec is warding off is
third-party/experimental forks and `3.0.0-*` prereleases if they appear. The Phase-0 guard
("Reject tauri-cef 3.x") is right in spirit; rewording to "reject any 3.x/pre-release
resolve of `tauri`" costs one line and survives the naming nit. Cosmetic.

### o3 — Pin the `tauri-cli` install (both dev machine and CI)

DoD-1 says `cargo tauri dev` and Phase-3 runs `trunk build`, but neither `tauri-cli` nor
`trunk` has a pinned install/version anywhere in the plan. Suggest one Phase-0 line:
`cargo install tauri-cli --version "^2"` (dev) and for CI either the same (slow) or
`taiki-e/install-action` (prebuilt) with the version recorded — otherwise the trunk-vs-CI
risk (§6 row 9 / `D-0110-ci-trunk`) conflates "trunk blocked" with "CLI never installed".

### o4 — Single-open overview: prefer `load_case_overview_on`

§3.3 says "Run `load_case_overview` + `Matter::open_for_read` / `info` on a blocking
worker". `load_case_overview_on(&Matter, opts)` exists (`overview.rs:302`) precisely so an
already-open `Matter` can be reused; the host should `Matter::open_for_read` once, call
`info()`, then `load_case_overview_on` — one WAL open instead of two. Same numbers, less
I/O; also matches the §2.2 note that `_on` is "sequential (tests)" i.e. callable on any
thread you own. Optional; either shape passes DoD-2.

### o5 — Pin-count drift is self-correcting

Spec §2.5 says "3886 pinned"; today's preflight reports **3887** (0109 fold-in decision added
since). Phase 0 re-verify covers it; recorded here for the drift log. Semantic recall and
embedding endpoints (8083) were reachable this pass — §2.5's tool notes stand.

## What looks solid

- **Every chip is grounded in a live struct field** — including the honesty-critical ones:
  `top_level_items` vs `items_total` (:73 vs :67 — the attachment trap is caught in the spec
  table), `privilege.claimed` as active claims (:104), `withhold` as a separate pill (:107),
  `errors.total` matter-scoped (:123), and the custodian remainder rule, which I verified
  inside `group_by_label_top_n` (:664-707: remainder = Σ all group counts − top-N sum, i.e.
  **items**, exactly what the tooltip must say).
- **The one-command lock survives contact with the API**: `CaseOverview`+`MatterInfo`
  (`overview.rs:158-175`, `matter.rs:118-124`) genuinely cover every locked chip with zero
  new SQL; `overview_by_custodian(top_n)` honors `top_n == 0` and orders `count DESC, label ASC`
  — deterministic for the UI.
- **Honesty rules have teeth**: Produced=`—` (no fake 0), no subject/body/path fields in the
  response, encrypted fail-closed *before* open (`is_encrypted_matter` at
  `crypto/header.rs:107` runs without a session), no passphrase UX (D-0063-05 untouched).
  `Matter::create` refuses on existing `matter.db` (:1087-1092), so `create_matter` can't
  silently reuse a folder.
- **Encrypted-path panic risk is correctly modeled**: `open_with_passphrase` /
  `open_inner` recover header temps first (:1272-1278, :1392-1404) and the env-passphrase
  path exists (:1405-1406) — the spec's "detect `is_encrypted_matter` first; never open" is
  the right contract, and the mock's `expect`-ful `run()` is explicitly forbidden (§3.6).
- **Workspace hygiene is correct and cheap**: `exclude` of `ui/` is the right (and only)
  answer to the wasm/`cargo test --workspace` risk; deny.toml already allows OFL-1.1 (:51) so
  the font plan is real; the mock's sins (CSP null, `.expect` run, `com.dedupe.review`,
  "Dedupe Review" product) are each named as forbidden in §2.3/§3.6 — I checked all four in
  the mock tree and the spec's "do not copy" list is exact.
- **Blast radius is small and fenced**: no `dedupe-desk`/`process-runner`/`pst-*` deps in the
  host crate list; `worse_cli_exit`-era unique-pst surface untouched; `cargo check -p
  dedupe-desk` keeps the egui app compiling; CI stable toolchain pin (all 6 jobs) blocks any
  accidental nightly for start-trunk.
- **ai-brains fold-in coherence**: the 0110 Ready decision (`5f7d3835`) matches the spec
  verbatim; the 0109 fold-in decision (`cb1480b7`) confirms Series O ordering and that 0109's
  residuals never leak into this track. No contradiction with stale "do 0108 first"
  memories — the spec explicitly supersedes them (§2.5).

## Deferred fold-in table

| Row | Live state (`docs/deferred.md`) | Spec disposition | Verdict |
|---|---|---|---|
| **D-0110-matter-chrome** | :918 — open / **0110 Ready — absorb on Implement** | Absorb / close | ✅ |
| D-0032-01 / D-0034-02 | :191 / :227 (+ :922-923) — residual, owner **0114** | Decline | ✅ |
| D-0040-01 / D-0060-04 | :328 / :626 (+ :924-925) — parked / **0115** | Decline | ✅ |
| D-0063-05 | :910 — residual polish (egui passphrase widgets) | Decline, no passphrase UX here | ✅ |
| D-0020-01 | :54 — operator polish (egui smoke), separate UX | Decline (owner HITL analog) | ✅ |
| D-0032 / D-0034 / D-0040 "do not close" | :922-925 all still owned by 0114/0115 | Decline | ✅ |
| D-0110-ci-trunk | mint-if-blocked (§9 tail) — **new row, matches DoD-5 escape hatch** | absorb-on-block | ✅ no new ID minted |
| Produced numeric / distinct-custodian SQL | 0113 / declined-without-new-ID | ✅ |

No open med/high row overlaps this surface; the only open row it *touches* (D-0063-05,
passphrase widgets) is explicitly declined. Next free ID **0117** stands (0111-0116 exist as
placeholders; spec's "no 0117 for custodian SQL" discipline is correct).

## Cursor / last-PR comments the plan missed

PRs **#110, #109, #108, #107** all merged (gh verified; #110 = docs 0109 Completed registry
`1272ff0`, #109 = 0109 classify/cancel honesty). `gh pr view 110/109` → **0 comments, 0
review bodies**. §2.8's "none" dispositions are correct; no new placeholder needed.

## Research / tools notes

- **ai-brains: used** from `C:\dev\Dedupe` — `preflight --summary` (inited; **3887** pinned;
  discovery-grants empty hint only), `sync query` + `recall --semantic` on 0110/Series O:
  decision `5f7d3835` recovered verbatim-matching the spec (crate layout, one command, chip
  map, chips honesty, fail-closed encrypted, Plex/paper, stable Tauri 2 + Leptos 0.8, no
  daemon, 0115 parked, no BCC, next 0117); 0109 completion decision `2ccf559d` confirms
  Series S closed and frontend next. No contradictions with spec/plan.
- **ledgerful: used from `C:\dev\Dedupe`** — `doctor --json` readyForPublish **true** (warns
  are the known standing five: phantom-promote legacy, sig-pin, sig-version, stale
  hook-template, plus optional 8081/8083 model-unreachable warnings; none block planning —
  recorded in prior reviews too); `ledger status --compact` **0 pending / 0 unaudited drift**;
  `scan --impact` **LOW** (dirty tree = conductor registry bumps + root `agy-review.md` +
  `.claude` junction; no product crates in the diff; unrelated file-budget warnings from
  `output/inc0102784-*` fixtures). Planning tx `c374a254` verified in ledger search.
- **Online research: applied** — crates.io API for tauri/leptos (pins verified current,
  table above) and v2.tauri.app CSP + configuration docs (basis of M1). MS-PST: N/A — the
  spec itself marks MS-PST "N/A this track" and no PST/NDB surface is touched. Tauri-Leptos
  official guide remains 0.6-era as spec §2.4 says (Trunk + `withGlobalTauri` + port 1420
  all re-confirmed against the mock's working config, which is the better source anyway).
- HITL: waiver is correct — every AC except the visual launch is CI-testable with tempfile,
  and the owner's "chips match Desk Overview" gate is owner-local by design (matches
  D-0020-01 analog precedent).

## Verdict: Ready after fixes

No B findings. Fold in before implement start:

1. **M1** — pin the CSP object (with `'wasm-unsafe-eval'` in `script-src`, `ipc:
   http://ipc.localhost` in `connect-src`, no Google Fonts entries) in §3.6, and add a
   one-clause DoD assertion (csp non-null + wasm-unsafe-eval). One token in prod config;
   otherwise the track fails at the HITL gate with a white screen and no test failure.
2. **m1** — rephrase `insert_source` as a pub API (usable from `cargo test -p dedupe-chrome`).
3. **m2** — pin env isolation for the encrypted-path test (`PST_DEDUPE_MATTER_PASSPHRASE`).
4. **m3** — pin recents ordering (MRU-promote), 20-cap eviction (truncate tail), and
   missing-root display (shown, errors `not_found` on open).
5. **m4** — pin `create_matter` root semantics: `root = parent.join(validated_name)`.
6. o1–o5 optional (round-trip encoding test, tauri-cli pin, single-open `load_case_overview_on`
   note; cosmetic wording on tauri-cef / pin-count drift).

`/foldin 0110` folds this file into spec/plan (fold review files only; do not implement here).