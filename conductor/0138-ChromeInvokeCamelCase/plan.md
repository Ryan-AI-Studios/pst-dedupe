# 0138 — Chrome invoke camelCase — Plan

> Status: **Completed**. Implement the UI choke point. Do not rename host commands.
> Fold-in 2026-09-24: `agy-review.md` + `opencode-review.md`.

> **Ledger:** open before edits —
> `ledgerful ledger start 0138-chrome-invoke-camel --category BUGFIX --message "CamelCase top-level Tauri invoke args"` —
> and commit it in the final phase.

---

## Phase 0 — Re-verify → DoD-1

- [ ] Re-read `crates/dedupe-chrome/src/lib.rs` command signatures and `ui/src/invoke.rs` `tauri_invoke`. Confirm still **no** `rename_all`, and `produce_start` still nests `WarningOverride` / `ChromeQcFinding` with snake_case fields on both sides.
- [ ] Confirm every UI `invoke` still goes through `tauri_invoke`. If a raw `invoke(` exists, route it through `tauri_invoke` or stop.
- [ ] Re-check the dirty diff. The WIP helper is incomplete (no array early-return, no Map branch). Keep it only as a starting point.
- [ ] **Revert** `ui/src/pages/process.rs` and `ui/src/pages/produce.rs` (`git checkout --` those two paths). The process.rs assert only matches the comment `paramsJson`. Do not edit `crates/dedupe-chrome/src/process.rs`.
- [ ] Re-read https://v2.tauri.app/develop/calling-rust/ (camelCase args; `rename_all` opt-in). If Tauri’s default changed, stop and update this plan before coding.

## Phase 1 — Rewriter → DoD-1 / DoD-2

- [ ] Pure walker (no `js_sys`), tested on the host: object keys camelCased; array / null / string / number / bool unchanged; nested object and array keys **not** renamed (`item_id` stays `item_id`).
- [ ] JS adapter in `tauri_invoke`, after `serde_wasm_bindgen::to_value` and **before** `invoke`:
  - null / undefined / non-object → unchanged
  - array → unchanged (before any `Object::keys` walk)
  - `js_sys::Map` → new plain object; entry keys through `to_camel_case_key`; values copied as-is (do not `Object.keys` a Map)
  - plain object → own keys only
- [ ] Do not recurse into values. Do not run the adapter on the `JsFuture` result. No `unwrap` / `expect`.
- [ ] Do not add the `heck` crate. `to_camel_case_key` tests: `params_json`→`paramsJson`, `job_id`→`jobId`, `filter_json`→`filterJson`, `item_ids`→`itemIds`, `source_entire_corpus`→`sourceEntireCorpus`, `warning_overrides`→`warningOverrides`, `page_index`→`pageIndex`, `root`→`root`, `paramsJson`→`paramsJson`, `""`→`""`, `"a"`→`"a"`, `item_id_2`→`itemId2`, `foo__bar`→`fooBar`, `job_`→`job`.
- [ ] `include_str` locks on the production section (split off `#[cfg(test)]`): one camel pass before `invoke`; result path has no second pass; array early-return; a `Map` branch; the walker is not called on copied values.

## Phase 2 — Host lock → DoD-3

- [ ] Add a `dedupe-chrome` unit test that reads every `*.rs` file under `crates/dedupe-chrome/src` (walk the directory; do not `unwrap`) and asserts none contain `rename_all`.
- [ ] Do not add `rename_all` to any command. Do not bump schema. Do not edit produce/process behavior.

## Phase 3 — Verify → DoD-4

- [ ] `cargo test -p dedupe-chrome --lib`
- [ ] `cargo test --manifest-path crates\dedupe-chrome\ui\Cargo.toml`
- [ ] `cargo fmt --manifest-path crates\dedupe-chrome\ui\Cargo.toml -- --check`
- [ ] `cargo check --manifest-path crates\dedupe-chrome\ui\Cargo.toml --target wasm32-unknown-unknown`
- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `ledgerful verify`
- [ ] UI crate is excluded from workspace fmt/clippy/test. UI fmt, UI tests, and the wasm32 check are mandatory even when the workspace gate is green. Do not add a UI clippy `-D warnings` gate.
- [ ] Write `review.md`. Set registry to **Completed**. Commit the ledger transaction. Note owner EXE HITL as not run unless it was run (synthetic matter: ingest, cancel, filtered queue, produce QC).

---

## Handoff notes

- Single-exe / no daemon stays. This track does not add a server.
- Do not commit client PSTs or `output/`.
- `git add -f` the new `conductor/0138-ChromeInvokeCamelCase/` files when the owner commits (conductor is gitignored).
- Do not combine this with a host `rename_all = "snake_case"` follow-up in the same PR.
- Rollback: revert the UI helper; host commands are unchanged so the previous snake_case bug returns, and nothing else moves.
