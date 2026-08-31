# Track 0115 ImageOptFactory — review

Engineering complete. Registry **Completed** and deferred closeout land in Phase 8 after squash-merge.

## Scope

Opt-in TIFF CCITT Group 4 + Opticon `IMAGE.opt` factory. Schema **v41**. Builtin `us_concordance_image_opt_v1` + `qc_image_opt_v1`. Default `us_concordance_native_text_v1` stays DAT-only.

Branch: `track/0115-image-opt-factory`  
PR / merge SHA: pending publish.

## DoD

| Item | Result |
|---|---|
| DoD-1 DAT-only unchanged | PASS — `dod1_dat_only_profile_no_images` |
| DoD-2 Image volume / G4 IFD / inbound TIFF / resume | PASS — produce integration + `pdf-raster` G4 oracles |
| DoD-3 Native-only xlsx / OPT omit | PASS — `dod3_native_only_xlsx_zero_opt_warn` |
| DoD-4 Burn token / `burned_native_missing` | PASS — image produce refuse + token tests |
| DoD-5 Schema 41, packs, CI, no forbidden deps | PASS locally (`fmt` / clippy / `cargo test --workspace`). `cargo deny` / chrome trunk in GitHub CI. |
| DoD-6 Recorded | Partial until Phase 8 (this file + registry Completed + deferred close) |
| Owner HITL | Not CI — release EXE smoke remains owner |

## Reviewer rounds

| Round | Verdict |
|---|---|
| Internal vs DoD | Fixed >low before Codex |
| Codex r1–r8 | FAIL — polarity, QC scope, resume/finalize fail-closed, schema claims, folder-cap orphans, DAT-only QC leak, multi-IFD TIFF burn collapse |
| Codex r9 (`review.codex.r9.md`, gpt-5.6-luna high) | **PASS WITH DEFERRED P3** — no open P0–P2 |

Accepted P3 residuals (already minted): **D-0115-lfp**, **D-0115-color**, **D-0115-email-print**.

## Local gates (orchestrator-observed)

- `cargo fmt --all --check` pass
- `cargo clippy --workspace --all-targets -- -D warnings` pass
- `cargo test --workspace` pass
- `ledgerful verify` alias timed out at 300s on `cargo test`; fallback workspace test already passed

## Out of scope (honored)

0116–0120 not implemented. No BCC-default. No unique-pst depth change. No `ITEM_COLUMNS` append. No `process-runner` on chrome. `ui/Cargo.toml` has no `fax`/`tiff`/`zpdf`. Produce uses `wrap_g4_le_ifd`, not `fax::tiff::wrap`.
