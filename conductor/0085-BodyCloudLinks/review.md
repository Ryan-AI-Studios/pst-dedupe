# 0085 — Body-Inline Cloud Link Detect — Review

**Status:** **Completed**  
**Date:** 2026-07-29  
**Verdict:** Codex gpt-5.6-luna high **PASS WITH DEFERRED P3** (final clean gate after FAIL→fix)

## Fold-in table (spec §2.10)

| # | Claim | Shipped |
|---|---|---|
| A1-1 | Excel `:x:` mandatory | Yes — action token set + unit tests |
| A1-2 | ReDoS closed by stock `regex` + fixed patterns + 100k window | Yes — workspace `regex` only; no `fancy-regex` |
| A1-3 | Sovereign-cloud residual | Yes — **D-0085-sovereign-cloud-hosts** opened; commercial allowlist only |
| A2-1 | Document-shaped only (no bare host) | Yes — miss bare `/sites/HR`; path-only action tokens |
| A2-2 | Never strip query params | Yes — href/SafeLinks exact; bare strips sentence punct only (not `?`/`:`) |
| A2-3 | Mode A body-only known gap | Yes — contract + runbook + zero-attach Mode A tests |

## Allowlist tokens

- Hosts: `*.sharepoint.com`, `*.sharepoint-df.com`, `onedrive.live.com`, `1drv.ms`, commercial SafeLinks
- Action (path only): `:w:` `:x:` `:p:` `:b:` `:u:` — exclude `:f:`
- Extensions: `.docx` `.doc` `.xlsx` `.xls` `.xlsm` `.pptx` `.ppt` `.pdf` `.csv`
- Library paths: `/_layouts/15/Doc.aspx`, `/download.aspx` when **exact** query keys imply document

## Evidence

- Scanner: `crates/dedup-engine/src/body_cloud_links.rs` + 26-unit matrix
- Wire: scan at prepare (`prepared_winner_from_canonical`) before write-path body move
- CSV: `export_body_cloud_links.csv`; messages column `body_cloud_link_count`
- Deferred: close **D-0084-body-cloud-links**; open **D-0085-sovereign-cloud-hosts**

## ReDoS-by-construction

Stock FA `regex` crate; patterns fixed at init (`OnceLock<Option<Regex>>`, degrade if invalid); body window 100k; no lookaround / no `fancy-regex`.

## Codex review rounds

1. **FAIL** — query trailing punct strip; query-only token FP; `userid=` vs `id=`; regex panic; Mode A test had healthy attach.
2. **PASS WITH DEFERRED P3** — all prior engineering findings resolved; only intentional sovereign residual.

## Known gaps

- Mode A does not prefer physical-attach peer over HTML-inline-only
- Sovereign-cloud host suffixes not in default allowlist (**D-0085-sovereign-cloud-hosts**)
- unique-eml full attach/body ledger CSV remains residual (D-0073-eml)

## Gates (orchestrator)

- `cargo fmt --all --check` PASS
- `cargo clippy` targeted + workspace (prior) PASS
- `cargo test -p dedup-engine -- body_cloud` 26 PASS
- `cargo test -p pst-dedup-cli --test unique_pst -- body_cloud` 3 PASS
- `cargo test --workspace` PASS (pre-final-fix full suite; re-run after fixes in CI)
- `cargo deny check` PASS (prior)
- Ledger FEATURE committed at merge time
