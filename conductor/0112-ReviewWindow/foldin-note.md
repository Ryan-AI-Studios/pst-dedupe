# 0112 fold-in (2026-08-30)

Sources: `opencode-review.md` + `agy-review.md`. Harness files not edited.

| Id | Disposition |
|---|---|
| opencode-M1 | Partial — host apply-then-upsert inside `review_window_apply`; no matter-core `ensure_item_privilege_conn` change |
| opencode-M2 | Fold — `family_members_thin` SQL LIMIT |
| opencode-m1 | Fold — `position` always counted |
| opencode-m2 | Fold — auto-claim documented; host pre-check |
| opencode-m3 | Fold — `insert_family` → parent → children |
| opencode-m4 | Fold — `include_on_log` default true; asserted-only |
| opencode-m5 | Fold — `cas_len` + prefix + `from_utf8_lossy` |
| opencode-m6 | Fold — copy Desk html_strip tests |
| agy-F-0112-1 | Already covered (catalog read-first) |
| agy-F-0112-2 | Already covered (sort-key neighbors) |
| agy-F-0112-3 | Covered + DoD-4 whitespace tighten |
| agy-F-0112-4 | Already covered (basis pre-check) |
| agy-F-0112-5 | Already covered (no innerHTML) |

Status remains **Ready — not started**. Ledger tx `bab0df96`.
