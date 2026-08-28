# 0104 fold-in — 2026-08-28

Sources (not edited): `opencode-review.md`, `agy-review.md`.

Status stays **Ready — not started**. No product crates.

| Id | Disposition |
|---|---|
| opencode-m1 | Fold — delete `build_attachment_table_tc` **and** `heap_data_len` |
| opencode-m2 / agy-0104-3 | Fold — DoD-2b names ≥20 BMP chars so `heap > 8176` is real |
| opencode-O1 | Partial — template leaf `47c336f7-2d9b-4f22-91c7-5bb422aaebbb` via toc.json; extra props optional |
| opencode-O2 | Fold — `add_subnode_leaf` doc-comment names 0104 |
| opencode-O3–O5, agy-0104-1/2/4 | Already covered |

Declined: extra attach-table columns; weakening `heap > 8176` to `>=`.
