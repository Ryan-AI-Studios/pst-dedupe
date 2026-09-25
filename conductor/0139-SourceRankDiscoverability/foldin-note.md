# Fold-in 0139 — 2026-09-25

Sources: `agy-review.md`, `opencode-review.md` (also bundled in `AI-review.md`).

Expanded placeholder spec/plan to **Ready — not started**. Did not implement.

Load-bearing decisions:

- Keep `first_seen` = sorted resolved paths.
- Add CLI-wrapper `input_path_sort_order`; keep unique-pst `inputs`.
- Oracle **removes** the new root key; do not allowlist the name.
- Stderr note when ≥2 inputs and no non-empty `--source-rank`. `--prefer-path-contains` does not suppress.
- Emit after `sort_input_paths` in the three command runners only (also-eml inner silent).
- Ledger **FEATURE**.
