# 0138 — ChromeInvokeCamelCase — Review

## Scope

Shallow top-level camelCase rewrite in `tauri_invoke` so Tauri 2.11.5 receives `paramsJson` / `jobId` / `filterJson`. Host commands stay default camelCase (no `rename_all`). Nested `WarningOverride` / `ChromeQcFinding` fields stay snake_case. Schema **41**.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Args | PASS | `js_args_to_camel` before `invoke`: array early-return, `js_sys::Map` via `map.entries()`, plain-object own keys. Nested values copied. `JsFuture` result not rewritten. |
| DoD-2 Keys | PASS | `to_camel_case_key` covers §3.4 including `item_id_2`→`itemId2`, `foo__bar`→`fooBar`, `job_`→`job`. Host-tested `shallow_camel_object_keys` keeps nested `item_id` and arrays. `include_str` locks array/Map/no recursion/one pass. |
| DoD-3 Host | PASS | `chrome_host_src_keeps_default_arg_case` walks `src/**/*.rs`. Schema 41. Comment-assert on `pages/process.rs` not shipped. rustfmt wrap of `process.rs`/`produce.rs` only so UI `fmt --check` passes (no behavior). UI wasm32 check pass. |
| DoD-4 Recorded | PASS | This file; registry **Completed**; **D-0138-chrome-invoke-camel** closed. Owner EXE HITL **not run**. |

## Gates

| Command | Result |
|---|---|
| `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` | 58 passed |
| `cargo test -p dedupe-chrome --lib` | pass (incl. host rename lock) |
| `cargo fmt --manifest-path crates/dedupe-chrome/ui/Cargo.toml -- --check` | pass |
| `cargo check --manifest-path crates/dedupe-chrome/ui/Cargo.toml --target wasm32-unknown-unknown` | pass |
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass |
| `ledgerful verify` | pass |

Internal review: PASS WITH DEFERRED P3 (Map lock tightened; UI fmt vs revert reconciled as wrap-only). Codex r1 FAIL on UI fmt + unrecorded DoD-4. Codex r2 **PASS** (no P0–P3).

## HITL

Owner release `dedupe-chrome` EXE (ingest start, pause/cancel, filtered queue, produce QC/start on a synthetic matter) **not run**.

## Publish

- PR / SHA: pending (filled after `gh pr create` / squash-merge)
- Closes **D-0138-chrome-invoke-camel**
