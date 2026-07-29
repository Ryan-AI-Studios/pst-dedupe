# 0081 — Implementation notes (draft audit)

**Research date:** 2026-07-29  
**Ledger tx (open):** `ba689f86-1539-47c0-b75e-a1550c376345`  
**Branch:** `track/0081-unique-export-deps-docs`

Orchestrator finalizes `review.md` + conductor flip after audit. This file is the implementer draft.

---

## Dependency audit table

| crate | workspace pin | lock (after) | crates.io max (spec §2.3 / re-query) | decision | notes |
|---|---|---|---|---|---|
| clap | `"4"` (loose) | **4.6.4** | ~4.6.4 | **PATCH** | Applied via `cargo update -p clap` (was 4.6.2) |
| serde_json | loose 1.x | **1.0.151** | ~1.0.151 | **PATCH** | Applied (was 1.0.149) |
| thiserror | product 2.x | **2.0.19** (+ dual 1.0.69) | ~2.0.19 | **PATCH** 2.x | Applied `thiserror@2.0.18` → 2.0.19; dual 1.x via openidconnect/oauth2 **ACCEPT_DUAL** |
| camino | loose | **1.2.5** | ~1.2.5 | **PATCH** | Applied (was 1.2.4) |
| uuid | loose 1.x | **1.24.0** | ~1.24.0 | **MINOR** | Applied (was 1.23.1); tests green expected |
| sha2 | product **0.11** | 0.11.0 (+ **0.10.9** dual) | 0.11.0 | **KEEP** product; **ACCEPT_DUAL** | Dual invert: `sha2@0.10.9` ← openidconnect/oauth2/ed25519-dalek/p256/p384 (SSO) + lopdf/pdf-extract |
| md-5 | product **0.10** | 0.10.6 (+ 0.11 dual residual) | 0.11.0 | **KEEP** 0.10 | EDRM MIH product pin; Q5 locked — no major |
| rusqlite | — | 0.40.1 | 0.40.1 | **KEEP** | |
| eframe | — | 0.34.2 | 0.35.x | **DECLINE_MAJOR** | Mid-RC GUI major |
| reqwest | product 0.12 | **0.12.28** active; **0.13.4** lock residual | 0.13.4 crates.io | **DECLINE_MAJOR** product 0.12 | Active default graph: desk + matter-ai + oauth2→openidconnect → **0.12.28** only. Lock also lists **0.13.4** as dep of `object_store` (matter-storage cloud path; not in default `cargo tree -i reqwest@0.13.4`). Not oauth2 dual. |
| aes-gcm | — | 0.10.3 | 0.11 | **DECLINE_MAJOR** | |
| argon2 | — | 0.5.3 | 0.5.3 (0.6-rc) | **KEEP** | |
| rand | multi-line | **0.8.7 / 0.9.5 / 0.10.2** | 0.10.2 | **KEEP** all lines | **RUSTSEC-2026-0097** INFO unsound when `log` + `thread_rng` + custom logger reseed; patched ≥0.8.6 / ≥0.9.3 / ≥0.10.1 — lock already past floors; do not force workspace unify mid-RC |
| chrono / csv / crc32fast / rfd / tantivy / object_store | current | current | current | **KEEP** | |

### Dual invert notes (`cargo tree -i`)

**sha2@0.10.9** (depth 3):

- `ed25519-dalek` → `openidconnect` → `matter-service`
- `oauth2` → `openidconnect` → `matter-service`
- `p256` / `p384` → `openidconnect`
- `lopdf` → `pdf-extract` → `extract-pdf`

**thiserror@1.0.69:** `oauth2` / `openidconnect` → `matter-service` (SSO opt-in).

**rand:** 0.8.7 ← matter-core + SSO; 0.9.5 ← proptest (dev); 0.10.2 ← lopdf. All past RUSTSEC-2026-0097 floors.

**reqwest (corrected 2026-07-29 re-verify):**
- **Active default graph:** only **0.12.28** via `dedupe-desk`, `matter-ai`, and `oauth2` → `openidconnect` → `matter-service`.
- **Lock residual 0.13.4:** listed under `object_store` (matter-storage cloud/S3 path). `cargo tree --workspace -i reqwest@0.13.4 --locked` matches no packages under default features — not selected by default product CLI graph.
- Prior wording “dual 0.13 via oauth2/openidconnect” was **incorrect** (oauth path is 0.12.28). **DECLINE_MAJOR** product pin 0.12; no security override.

### deny.toml

**Pruned** dead `advisory-not-detected` ignores: RUSTSEC-2026-0186, 0190, 0194, 0195.  
**Kept live:** RUSTSEC-2023-0071 (rsa / openidconnect), RUSTSEC-2026-0192 (ttf-parser / lopdf).  
`cargo deny check` green without advisory-not-detected warnings for pruned IDs.

---

## Basename (`--ledger-path-mode`)

- Enum `LedgerPathMode { Full, Basename }` + `format_ledger_source_path` in `unique_export_report.rs`.
- CLI flag default **`full`**; threaded through `UniquePstClapArgs` / `UniquePstCliArgs` / GUI wizard default Full.
- Applied at **CSV serialization only**:
  - `write_export_messages_csv(..., path_mode)`
  - `AttachLedgerSink` → `ledger_row_from_event(..., path_mode)` for attach ledger CSV
- In-memory `ExportMessageRow.source_path` and msg_fail_counts keys remain **full** for QC join during the same run.
- `source_id` resolution always uses full event path.

---

## Timing script

`scripts/unique-pst-timing.ps1` — parameterized `-InputPaths`, `-Out`, `-ReportDir`, optional `-PstDedupExe`, `-RunScanFirst`, `-ExtraArgs`, `-TimingJson`. No hardcoded client paths.

---

## Docs

- New: `docs/unique-pst-ediscovery-runbook.md` (§2.10 sections 0–11)
- Links: README, `operator-golden-path.md` Path B, `unique-pst-export.md`
- Deferred: D-0073-basename closed; D-0077-repair-diff docs-closed; D-0078-retryable residual code + runbook constraint

---

## Hygiene

Accident mangled-path dirs under repo root matching `devdedupe`: **absent** (confirmed 2026-07-29).
