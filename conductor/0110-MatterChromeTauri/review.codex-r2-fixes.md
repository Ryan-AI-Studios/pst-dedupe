# 0110 Codex r2 fixes

Branch: `track/0110-matter-chrome-tauri`  
Registry remains **In progress** (no Completed / ledger commit / PR).

| ID | Finding | Disposition |
|---|---|---|
| **P2** | Double-decode of router `:id` broke roots with literal `%` | **Fixed** — `ParamsMap` values treated as already-decoded absolute roots. `MatterHome` uses param as root (no `decode_matter_id`). `matter_home_href_from_param` only `encode_matter_id`. Host + UI regression tests for `100%20Done` / `%25` (assert second decode would mutate). `decode_matter_id` retained for intentional decode of encoded strings/tests. |
| **P3** | Skip links `#matters` / `#counts` missing on most routes | **Fixed** — `<main id="main-content" tabindex="-1">` always present. Skip links keep labels; click focuses preferred id when mounted else `#main-content`. List keeps `#matters`; home keeps `#counts`. |
| Hygiene | Untracked `fixtures/keep_set_summary.json` | **Deleted**. |

## Verify

- `cargo fmt --all --check` — pass  
- `cargo clippy -p dedupe-chrome --all-targets -- -D warnings` — pass  
- `cargo test -p dedupe-chrome` — **18 passed**  
- UI `path_id` tests — **4 passed**  
- `trunk build --release --config crates/dedupe-chrome/ui/Trunk.toml` — success  
