# 0110 Codex r3 fixes

Branch: `track/0110-matter-chrome-tauri`  
Registry remains **In progress** (no Completed / ledger commit / PR).

| ID | Finding | Disposition |
|---|---|---|
| **P3** | `#matters` / `#counts` not programmatically focusable for skip links | **Fixed** — `tabindex="-1"` on list `<section id="matters">` and home `<div id="counts">`. `focus_skip_target` falls back to `#main-content` if preferred focus fails. |

## Verify

- `cargo test -p dedupe-chrome`
- `trunk build --release --config crates/dedupe-chrome/ui/Trunk.toml`
- fmt / clippy on touched UI as needed
