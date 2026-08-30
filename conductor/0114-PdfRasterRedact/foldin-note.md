# 0114 fold-in (2026-08-30)

Sources: `opencode-review.md` + `agy-review.md`. Harness files not edited.

| Id | Disposition |
|---|---|
| opencode-M1 | Fold — `burned_source_digest` includes 0032 text state; DoD-3 recode variant |
| opencode-M2 / agy-M2 | Fold — `redact_page` → `write(cursor)` → `PdfFile::parse` → `rewrite_pdf`; `iw.document()` forbidden |
| agy-M1 | Fold — host CropBox/`/Rotate` map; Rotate-90 DoD |
| agy-M3 | Fold — `matter-qc` Error `burned_native_missing` + unmapped-text on default pack |
| opencode-m1 | Fold — RGBA → PNG via `image` |
| opencode-m2 | Fold — JPEG burn stays JPEG / `FILE_EXT=jpg` |
| opencode-m3 / agy-m2 | Fold — Image-tab `generation` minted here; 0118 keeps document/body |
| opencode-m4 | Fold — `search_spans` line-bounded; zero-hit → unmapped |
| opencode-m5 | Fold — DisplayList construction in Phase 0 |
| agy-m1 | Fold — append v40 columns at end of `ITEM_COLUMNS` |
| agy-O1 | Fold — 1 pt hit dilation |
| opencode-O1 | Fold — `cargo test -p matter-core` |
| opencode-O2 | Fold — prefer `EncryptionConfig` fixture |

Status remains **Ready — not started**. Ledger tx `57006a8f-7bf4-4c00-8f3b-0366c89fc34f`.
