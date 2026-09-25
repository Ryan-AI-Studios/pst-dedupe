# 0138 — Chrome invoke camelCase (Tauri 2 IPC)

> Finish the uncommitted `tauri_invoke` key rewrite so chrome commands with underscored
> arguments actually reach the host. **Shallow only.** Nested produce findings stay snake_case.

- **Track ID:** 0138-ChromeInvokeCamelCase
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` (chrome is the review window; Desk stays single-exe, no daemon)
- **Status:** Completed
- **Depends on:** Series O–V chrome **Completed** (host commands in `crates/dedupe-chrome/src/lib.rs`; UI `tauri_invoke` in `crates/dedupe-chrome/ui`)
- **Spec authored:** 2026-09-24 Ready (HEAD `73f1d7f`)
- **Fold-in:** 2026-09-24 `agy-review.md` + `opencode-review.md` (still Ready — not started)
- **Series:** W
- **Ledger category:** `BUGFIX`

> **Closes / absorbs:** `D-0138-chrome-invoke-camel` (minted with this plan).
> **HITL (owner, not CI):** release `dedupe-chrome` EXE — ingest start, pause/cancel, a filtered review queue, and produce QC/start on a **synthetic** matter. No INC\* PST in git. No codesign (D-0062-codesign stays).

---

## 1. Objective

Tauri 2 looks up command arguments in **camelCase** (`paramsJson`, `jobId`, `filterJson`). The Leptos UI serializes snake_case struct fields and the host has no `rename_all`. One-word args (`root`, `kind`, `name`) still work. Every underscored argument is dropped, so Process start, cancel/resume, queue filters, coding, raster, and produce QC/start do not pass the fields the host requires.

Deliver one choke-point rewrite inside `tauri_invoke`: top-level keys only, before `invoke`. Do not camelCase return payloads or nested struct fields.

This is chrome correctness (jobs and review actually receive their arguments), not unique-export work and not UI polish.

---

## 2. Context (read before starting)

### 2.1 Live facts (plan-time `73f1d7f`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| Tauri | `Cargo.lock` **tauri 2.11.5**. Official contract: JS keys are camelCase unless `#[tauri::command(rename_all = "snake_case")]`. https://v2.tauri.app/develop/calling-rust/ — **re-verify** the doc sentence at execute. |
| Host | `crates/dedupe-chrome/src/lib.rs`: **32** `#[tauri::command]` fns. **Zero** `rename_all` anywhere under `crates/dedupe-chrome`. |
| UI | `crates/dedupe-chrome/ui` is workspace-**excluded**. All commands go through `tauri_invoke` (`ui/src/invoke.rs`). Args are `serde` structs (snake_case fields). `serde_wasm_bindgen` 0.6 turns structs into plain objects and `serde_json::Value` objects into JS `Map`s. |
| Responses | Host `Serialize` structs have **no** `rename_all`. UI `Deserialize` fields are snake_case (`job_id`, `ordered_ids`, `item_id`, …). Return JSON must stay snake_case. |
| Nested command values | `produce_start` takes `warning_overrides: Option<Vec<WarningOverride>>` and `last_findings: Option<Vec<ChromeQcFinding>>`. Host and UI field names match (`item_id`, `rule_id`, `qc_run_id`, `recorded_by`). Tauri camelCases only the **parameter name**, not nested serde fields. |
| Strings that are JSON | `params_json` and `filter_json` are opaque `String`s. Do not parse or rename inside them. |
| Schema | `SCHEMA_VERSION` **41**. No bump. |
| MS-PST | **N/A this track.** |
| CI | See the `chrome-ui` row above. |
| WIP on the planning tree | Uncommitted: `invoke.rs` (null/non-object guard + `Object::keys` only — **no** array early-return, **no** `Map` branch), `process.rs` (fmt + `contains("paramsJson")`, which matches a comment), `produce.rs` (**fmt only**). Re-verify the diff. The helper is **not** done. **Revert both** `pages/process.rs` and `pages/produce.rs`. Keep tests in `invoke.rs`. |
| CI `chrome-ui` | Trunk `wasm32-unknown-unknown` build, then `cargo test -p dedupe-chrome`, then `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml`. No UI `fmt` / `clippy`. Workspace `fmt` / `clippy` / `test` do not compile `ui/`. |

Underscored host parameters that must arrive as camelCase keys (plan-time; re-list at execute if a command was added):

| Command | Snake keys the UI sends today |
|---|---|
| `process_start` | `params_json` |
| `process_cancel`, `process_resume` | `job_id` |
| `produce_qc_findings` | `job_id` |
| `review_queue_page`, `review_document` | `filter_json`, `item_id` (document) |
| `saved_search_upsert` | `filter_json` |
| `review_codes_preview` | `item_ids`, `add_code_ids`, `remove_code_ids` (no `propagate_family`) |
| `review_apply_codes` | those three plus `propagate_family` |
| `review_window_apply` | those plus `privilege_basis`, `include_on_log`, `privilege_description` |
| `review_document_body`, `review_upsert_note`, `review_upsert_privilege` | `item_id` |
| `review_raster_page` | `item_id`, `page_index` |
| `review_geom_list`, `review_geom_from_hits`, `review_burn_native` | `item_id` |
| `review_geom_upsert` | `item_id`, `page_index`, `raster_width`, `raster_height` |
| `review_geom_delete` | `geom_id` |
| `produce_burn_set` | `item_ids` |
| `produce_qc_run` | `filter_json`, `item_ids`, `production_profile`, `source_entire_corpus` |
| `produce_start` | those plus `bates_prefix`, `bates_start`, `warning_overrides`, `last_findings`, `log_format` |

`State<ProcessRunner>` is not a JS argument. One-word params (`root`, `kind`, `name`, `pane`, `body`, `id`, `query`, `reason`, `label`, `source`, `generation`, `dpi`, `px`, `py`, `pw`, `ph`, `keyword`, `limit`, `offset`, `extras`, `withhold`) are unchanged by the rewrite.

### 2.2 Tools (plan-time)

| Tool | Result |
|---|---|
| `ai-brains preflight --summary` | Vault live (project `93e74c21`). No prior decision on invoke casing. |
| `ai-brains recall` / `sync query` | Chrome memories are 0110–0112/0118 (one `matter_overview`, UI tests via the excluded manifest). No camelCase decision to conflict with. |
| `ledgerful doctor --json` | Ready (0 block). Stale hook template is unrelated; do not refresh hooks in this track. |
| `ledgerful ledger status --compact` | 0 pending, 0 unaudited drift. |
| `ledgerful scan --impact` | Dirty tree. Saved `latest-impact.json` couples `ui/src/pages/process.rs` and `invoke.rs` to **directories** at score 1.0. It does not record an 82% file-to-file edge with `crates/dedupe-chrome/src/process.rs` (the planning note that cited 82% is dropped). Git log on this tree still co-lands the two `process.rs` files (shared commits include `a8287b4`, `73c0496`, `fce416e`, `f1810fe`, `727c857`). That is why host `process.rs` stays unread-for-edit. Do **not** edit `unique_pst_cmd.rs` or `dedupe-desk`. UI `produce.rs` is a hotspot — revert the fmt-only diff. |

### 2.3 Locks

- Host commands stay default camelCase. **Do not** add `rename_all = "snake_case"` on the same change (double rename).
- Rewrite is **shallow**. Copy nested objects and arrays as values. A deep walk turns `WarningOverride.item_id` into `itemId` and produce start drops overrides/findings.
- Do not rewrite the `invoke` **return** value.
- Schema **41**. No BCC-default. No password vault. No OST/MBOX. No unique-pst / unique-eml edits. Desk egui process stays.
- No `unwrap` / `expect` in the new helper.

---

## 3. In scope

1. Harden `tauri_invoke` so the value passed to `invoke` is a plain object whose **own** keys are camelCase.
2. Split the rewrite:
   - A **pure** shallow walker (no `js_sys`) that host `cargo test` actually runs: object → rename own keys; array / null / string / number / bool → unchanged; nested objects and arrays copied **without** renaming their keys.
   - A thin JS adapter: `Array` returned as-is; `js_sys::Map` entries copied into a new plain object (do not use `Object.keys` on a Map); plain object own keys only. Values are not passed back through the walker.
3. The only `serde_json::Value` args call today is `recent_matters_list` with `json!({})` (an empty Map). `Object.keys` on that empty Map already yields `{}`. The Map branch is still required so a later non-empty `Value` arg is not wiped. It is future-proofing, not a live payload.
4. `to_camel_case_key` tests: §2.1 keys, `root`, an already-camel key, `""`, `"a"`, `item_id_2` → `itemId2`, `foo__bar` → `fooBar`, `job_` → `job` (no panic). The on-disk WIP test (five keys) is not this list.
5. Pure-walker tests: top-level `params_json` becomes `paramsJson`; a nested `item_id` inside an object or array **stays** `item_id`; an array value is not turned into `{"0":…}`.
6. `include_str` locks on the adapter: exactly one camel pass, and it is **before** `invoke`; the `JsFuture` result is not passed through it; array early-return; Map branch; no recursive call on values.
7. Host test: every `*.rs` file under `crates/dedupe-chrome/src` contains no `rename_all` (not only `lib.rs`, and not only the exact token `#[tauri::command(rename_all`). All 32 commands are in `lib.rs` today; the scan is so a later submodule command cannot slip a snake_case rename in.

## 4. Out of scope

- `#[tauri::command(rename_all = "snake_case")]` as the fix (declined; one UI choke point instead).
- camelCase on response structs or a second pass on the invoke result.
- Walking into `params_json` / `filter_json` strings or into `WarningOverride` / `ChromeQcFinding` fields.
- Host `process.rs` behavior, produce math, 0119 latch, 0125 canvas, 0122 Busy.
- Behavior edits in `pages/process.rs` or `pages/produce.rs`. Revert both dirty diffs. Do not restyle them.
- D-0125-dead-css, D-0116-workflow, D-0110-deny-unic, image/PDF residuals.
- Hook-template refresh from `ledgerful doctor`.

## 5. Preconditions

- **P1:** `tauri_invoke` remains the only UI invoke path. If a new raw `invoke(` appears outside it, fold that call into `tauri_invoke` or stop and say so. Do not add a second converter.
- *Verified 2026-09-24:* 32 commands, no `rename_all`, nested produce structs are snake_case on both sides, UI crate tests are the `chrome-ui` job.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Deep camelCase breaks `produce_start` overrides/findings | Shallow copy of values. Test/lock that the rewriter does not call itself on values. |
| `Object.keys` on a Map yields `{}` | Explicit `js_sys::Map` branch. |
| Array args become `{ "0": … }` | Early-return when `Array::is_array`. |
| Someone later adds host `rename_all = "snake_case"` | Host test scans every `src/**/*.rs` for the substring `rename_all`. |
| WIP discarded before execute | Implement the same contract from §3. Do not invent a host-side rename. |
| Editing hot `produce.rs` / host `process.rs` | Revert fmt-only `produce.rs` and the `process.rs` comment-assert. Do not edit host `process.rs`. |
| Host tests never run `js_sys` | Pure shallow walker (below) is what `cargo test` executes. The JS adapter stays thin and source-locked. Trunk / `cargo check --target wasm32-unknown-unknown` typechecks the adapter. |
| `to_camel_case_key` vs heck | Tauri 2.6.3 macros use `heck` 0.5 `ToLowerCamelCase` (word boundaries, not a raw underscore scan). Do **not** add `heck` to the UI crate. Lock the helper for Rust snake identifiers we ship (`params_json`, `item_id_2` → `itemId2`, `foo__bar` → `fooBar`, `job_` → `job`). A leading underscore is not a host parameter and is not required to match heck. |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Args:** `tauri_invoke` rewrites top-level plain-object and Map keys to camelCase and passes that object to `invoke`. Arrays, null, and non-objects pass through. Nested values are not rewritten. Return values are not rewritten.
- [ ] **DoD-2 — Keys:** `to_camel_case_key` tests cover the §3.4 list, including `item_id_2` → `itemId2`, `foo__bar` → `fooBar`, and `job_` → `job`. The pure walker tests in §3.5 pass on the host (nested `item_id` unchanged; arrays not turned into objects). Source locks cover array skip, Map copy, no recursion, and no rewrite of the invoke result.
- [ ] **DoD-3 — Host stays default camelCase:** no `rename_all` under `crates/dedupe-chrome/src`. Workspace test scans those files. Schema stays 41. Dirty `pages/process.rs` and `pages/produce.rs` are reverted (no behavior change). UI `fmt --check` and `cargo check --target wasm32-unknown-unknown` pass for `crates/dedupe-chrome/ui`.
- [ ] **DoD-4 — Recorded:** `review.md`; registry **Completed**; ledger `BUGFIX` committed. Owner HITL called out as not run unless the operator did it.

## 8. Verification commands (reference)

```powershell
cargo test -p dedupe-chrome --lib
cargo test --manifest-path crates\dedupe-chrome\ui\Cargo.toml
cargo fmt --manifest-path crates\dedupe-chrome\ui\Cargo.toml -- --check
cargo check --manifest-path crates\dedupe-chrome\ui\Cargo.toml --target wasm32-unknown-unknown
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

UI crate is excluded from the workspace fmt/clippy/test jobs. The UI fmt check, UI tests, and wasm32 check are mandatory. CI `chrome-ui` already runs Trunk (wasm) plus `cargo test -p dedupe-chrome` and the UI manifest tests; it does not run UI fmt. Workspace clippy does not lint `ui/`; this track does not add a new UI clippy `-D warnings` gate.

## 9. Deferred roll

| Row | Disposition |
|---|---|
| D-0138-chrome-invoke-camel | **Absorb** (this track) |
| D-0110-deny-unic | **Decline** — upstream `unic-*` via Tauri; not argument casing |
| D-0116-workflow | **Decline** — no workflow picker |
| D-0116-drop / D-0133–D-0137 | **Decline** — closed in 0133–0137; do not reopen |
| D-0125-dead-css / D-0125-pad-fallback | **Decline** — do not edit `app.css` or Bates pad |
| D-0114-pdfium-sidecar / D-0114-xform-text / D-0115-* | **Decline** — raster/OPT residuals |
| BCC-default | **Decline** — 0082 opt-in stays |
| Host `rename_all = "snake_case"` | **Decline** as the implementation — UI choke point only |
| Response camelCase | **Decline** — responses stay snake_case |
| Nested rename inside `params_json` / findings | **Decline** — would break produce start |
| PRs **#148 #149 #150 #151** Cursor/Bugbot | **Decline** — no inline comments. Each issue thread is only “Bugbot couldn't run - usage limit reached” (`cursor[bot]`). No new placeholder. |
| Ledgerful hook-template-stale | **Decline** — not this bug |
| `ledgerful scan --impact` HIGH | **Decline as a work list** — directory coupling, not a mandate to edit unique-pst or Desk |
| D-0062-codesign | **Decline** — owner HITL stays unsigned; this track does not codesign |
| agy-F-01 / opencode-M2 pure `js_sys` execution | **Partial** — host tests run a pure shallow walker. Do not require `wasm-bindgen-test` or a prescribed `Map::entries` call shape |
| agy-F-02 / opencode-M4 `rename_all` only in `lib.rs` | **Fold** — scan all `src/**/*.rs` for `rename_all` |
| opencode-M4 pairwise UI field vs host param | **Decline** — the rewriter is generic; a pairing generator is a second framework |
| agy-F-03 `process.rs` comment assert | **Fold** — revert `pages/process.rs` with `produce.rs` |
| agy-F-04 / opencode-M1 wasm + UI fmt | **Fold** — UI `fmt --check` and wasm32 `cargo check`. UI clippy `-D warnings` stays out (not in CI; would sweep the whole CSR crate) |
| agy-F-05 / opencode-m2 key edges vs heck | **Partial** — lock `""`, `"a"`, `item_id_2`, `foo__bar`, `job_`. Do not add a `heck` dependency |
| opencode-M3 “82% is fabricated / intersection 0” | **Partial** — drop the 82% citation (absent from saved impact JSON). Git log intersection is **not** 0 (five shared commits). Host `process.rs` stays out of scope anyway |
| opencode-m1 WIP helper called shallow-but-done | **Fold** — §2.1 says the WIP has no array or Map branch |
| opencode-m3 `propagate_family` on preview | **Fold** — table split; preview has no `propagate_family` |
| opencode-m4 empty Map | **Fold** — Map branch kept as future-proofing; only `json!({})` exists today |
| opencode-O1 `D-0062-codesign` missing from §9 | **Fold** — decline row above |
| opencode-O2 CI under-described | **Fold** — `chrome-ui` also runs `cargo test -p dedupe-chrome` |
| opencode-O3 WIP test covers five keys | **Already covered** — DoD-2 list is the contract, not the WIP test |

No related open deferred row was skipped in silence. Med/high items in `docs/deferred.md` are not parked here.
