# 0110-MatterChromeTauri — Review

- **Track:** `0110-MatterChromeTauri`
- **Branch:** `track/0110-matter-chrome-tauri` (product) → `docs/0110-completed` (registry)
- **Registry:** **Completed**
- **Product PR:** **#111** squash-merged to `main` as `5a76f0b`
- **Docs PR:** records this Completed registry (after product merge)

---

## Definition of Done (§7)

| DoD | Status | Evidence |
|---|---|---|
| **DoD-1 — Crate + EXE** | **PASS** (engineering) | Workspace member `crates/dedupe-chrome`; `ui/` excluded; identifier `com.dedupe.desk.chrome`; title/product **Dedupe Desk**; window 1440×900 min 1024×640; four tabs on matter home; no daemon; `dedupe-desk` still checks. Owner release-EXE HITL is residual (below). |
| **DoD-2 — Overview honesty** | **PASS** | `matter_overview`: `metadata` → `not_found` / `failed`; `is_encrypted_matter` first (no `open_*`); else one `open_for_read` + `info` + `load_case_overview_on`. Chip map locked (Processed=`top_level_items`; Privileged=`claimed`; Withhold separate; Produced=`—`/`0113`). Host tests: empty zeros; `insert_source` Sources=1 & Processed=0; encrypted kind; CSP `'wasm-unsafe-eval'`. |
| **DoD-3 — List + create** | **PASS** | Recents MRU-front, load+remember cap 20, missing roots retained, injectable dir; create `parent.join(validated_name)` → `<parent>/<name>/matter.db`; invalid name rejected; remember failures surfaced (shell status) without silent discard. |
| **DoD-4 — Tokens + a11y + CSP** | **PASS** | Plex/paper tokens; no `#ec3013`; self-hosted OFL woff2; skip links + `:focus-visible`; `#matters`/`#counts`/`#main-content` focusable (`tabindex="-1"`); Windows Ctrl+K; CSP object with `'wasm-unsafe-eval'` + IPC `connect-src`; no Google Fonts. |
| **DoD-5 — Tests + CI** | **PASS** | `cargo test -p dedupe-chrome` **18 passed**; fmt / clippy `-D warnings` (crate); workspace gates run during implement; CI `chrome-ui` (wasm + trunk 0.21.14 + host tests); deny proprietary exceptions; no production `unwrap`/`expect`; `main` returns `Result`. |
| **DoD-6 — Recorded** | **PASS** | Product PR **#111** / `5a76f0b`. Registry **Completed**; `D-0110-matter-chrome` closed. Residual **D-0110-deny-unic** + Owner HITL EXE (external). |

---

## Internal review rounds

| Round | Open | Outcome |
|---|---|---|
| 1 | **8** (1 bug stub `:id` encode + suggestions/nits) | Fixed (encode helper, comments, trim, Ctrl+K Once, thiserror, trunk pin, icons) |
| 2 | **1** (Ctrl+K silent swallow off-list) | Fixed (focus only when `#matter-search`; visible hint otherwise) |
| 3 | **0** | Approve path for engineering DoD-1..5 |

---

## Codex completion audits

| Round | Verdict | Notes |
|---|---|---|
| r1 | **FAIL** (5× P2) | Recents load truncate; metadata `not_found`/`failed`; remember errors; host `path_id` CI; no temp recents fallback — all fixed (`review.codex-fixes.md`) |
| r2 | **FAIL** (1× P2 + 1× P3) | No ParamsMap double-decode; skip-link landmarks — fixed (`review.codex-r2-fixes.md`) |
| r3 | **FAIL** (1× P3) | `#matters`/`#counts` `tabindex="-1"` — fixed (`review.codex-r3-fixes.md`) |
| **r4** | **PASS** | `review.codex.r4.md` — no open findings |

**Open engineering findings > low:** none.

---

## Residuals (deferred / external)

| Item | Notes |
|---|---|
| **Owner HITL** | Launch **release** EXE against synthetic matter; chips match empty Desk Overview. INC* unique-pst **waived**. Codesign = D-0062 (not this track). |
| **D-0110-deny-unic** | P3 residual — unic-* unmaintained via tauri-utils/urlpattern; denied ignores remain. |

---

## Pins / stack (re-verified at implement)

- `tauri` **2.11.5** (`tauri = "2"`); `leptos` **0.8.20** / `leptos_router` **0.8.15** (not 0.9-beta)
- `SCHEMA_VERSION` **39**; CI `dtolnay/rust-toolchain@stable`
- Tools: `tauri-cli` 2.11.4; `trunk` **0.21.14** (CI pinned)

---

## Conductor files requiring `git add -f`

`conductor/` is gitignored — force-add when the owner commits the track:

```
conductor/0110-MatterChromeTauri/spec.md
conductor/0110-MatterChromeTauri/plan.md
conductor/0110-MatterChromeTauri/review.md
conductor/0110-MatterChromeTauri/review.codex.md
conductor/0110-MatterChromeTauri/review.codex.r2.md
conductor/0110-MatterChromeTauri/review.codex.r3.md
conductor/0110-MatterChromeTauri/review.codex.r4.md
conductor/0110-MatterChromeTauri/review.codex-fixes.md
conductor/0110-MatterChromeTauri/review.codex-r2-fixes.md
conductor/0110-MatterChromeTauri/review.codex-r3-fixes.md
conductor/0110-MatterChromeTauri/foldin-note.md
conductor/0110-MatterChromeTauri/opencode-review.md
conductor/0110-MatterChromeTauri/agy-review.md
conductor/conductor.md
conductor/ROADMAP.md
conductor/sequencing.md
```

(Adjust if some planning-only files are omitted from the product PR; **at minimum** force-add `spec.md`, `plan.md`, `review.md`, and Codex r4 + fix notes.)
