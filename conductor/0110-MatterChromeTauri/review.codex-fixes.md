# 0110 Codex P2 completion-audit fixes

Branch: `track/0110-matter-chrome-tauri`  
Registry remains **In progress** (no Completed / ledger commit / PR).

| ID | Finding | Disposition |
|---|---|---|
| **P2-1** | `recent_matters_list_in` returned raw file without MAX_RECENTS truncate | **Fixed** — `normalize_matters` truncates tail on load; test `list_truncates_oversized_file_on_load` seeds 25-entry JSON and asserts len 20 with tail gone. |
| **P2-2** | `Path::exists()` mapped any inaccessible root toward missing | **Fixed** — `fs::metadata` + `map_root_metadata_err`: `NotFound` → `not_found`, other IO → `failed`. Unit tests for both ErrorKinds (PermissionDenied without admin). |
| **P2-3** | UI `let _ =` ignored remember failures | **Fixed** — match remember result; on Ok refresh recents; on Err set list error + shell `#chrome-status` banner (survives navigate); **still navigate** (best-effort persist). |
| **P2-4** | UI `path_id` tests not in CI host gate | **Fixed** — duplicated pure encode/decode + stub-href helpers/tests in host `src/path_id.rs`; covered by `cargo test -p dedupe-chrome` (17 tests). UI module retained for CSR. |
| **P2-5** | `production_recents_dir` fell back to `temp_dir` | **Fixed** — returns `Result`; `None` from `data_local_dir` → structured `failed`. Commands use `?`. No silent temp writes. |

## Verify

- `cargo fmt --all --check` — pass  
- `cargo clippy -p dedupe-chrome --all-targets -- -D warnings` — pass  
- `cargo test -p dedupe-chrome` — **17 passed**  
- `trunk build --release --config crates/dedupe-chrome/ui/Trunk.toml` — success  
