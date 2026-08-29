# 0110 fold-in — 2026-08-29

Sources (not edited):

- `opencode-review.md`
- `agy-review.md`

Status stays **Ready — not started**. No product crates.

| Id | Disposition |
|---|---|
| opencode-M1 CSP `'wasm-unsafe-eval'` + IPC connect-src | Agree — fold (§3.6, DoD-4, Phase 2/4 HITL on **release** EXE) |
| opencode-m1 `insert_source` pub API | Agree — fold (§2.2) |
| opencode-m2 passphrase env flake | Agree — partial (never `open_*` on encrypted; assert kind `encrypted`) |
| opencode-m3 recents MRU/20/missing | Agree — fold (§3.3, DoD-3) |
| opencode-m4 `parent.join(name)` | Agree — fold (§3.3, DoD-3) |
| opencode-o1 `:id` encode round-trip | Agree — fold (Phase 2) |
| opencode-o2 reject `tauri` 3.x/pre | Agree — fold (Phase 0) |
| opencode-o3 pin `tauri-cli` ^2 | Agree — fold (Phase 0) |
| opencode-o4 `load_case_overview_on` | Agree — fold (required command shape) |
| opencode-o5 vault pin-count | Decline |
| agy-F-0110-1 ui exclude | Already covered |
| agy-F-0110-2 deny exception | Already covered |
| agy-F-0110-3 injectable recents dir | Agree — fold |
| agy-F-0110-4 empty-matter false-pass | Agree — partial (Sources=1 **and** Processed=0 after `insert_source`; no item insert) |
| agy-F-0110-5 no `expect` in main | Already covered |

No new conductor ID. No BCC-default. `D-0110-ci-trunk` still mint-if-blocked only.
