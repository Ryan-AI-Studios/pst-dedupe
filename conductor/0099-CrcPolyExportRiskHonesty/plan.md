# 0099 — CRC / Poly Export-Risk Honesty — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Phase 0 closed 2026-08-26.** Dual-rate stays the classifier. Thresholds key on
> **effective** (non-poly) `block_crc_read_rate`. Attach-stream CRC discounted only
> when every CRC-noisy source is poly-class. Vocabulary / monotone / no repair
> unchanged. See `spec.md` §3.
>
> **Review fold-in (2026-08-26):** `opencode-review.md` + `agy-review.md`. Disposition
> `spec.md` §2.8. Mapper + table-driven §3.4 + oracle attest pointers are DoD.
> `saturating_add` locked. Per-event attach CRC stays declined.

> **Ledger (planning):** `13df99b7-82a8-4a96-a0fc-8aa9c846d6ab` (Ready spec) ·
> fold-in `2afe7d13-a092-4230-a1ce-014813cfd611` (`crates/pst-dedup-cli`, `DOCS`).
>
> **Ledger (implementation, start at GO):**
> `ledgerful ledger start crates/pst-dedup-cli --category BUGFIX --message "0099 CRC poly export-risk honesty"`
> Commit summaries must also name `unique_export_report` + `unique_pst_cmd`.

---

## Phase 0 — Policy lock → DoD-1 (docs)

- [x] Map 0077 dual-rate vs unique-pst `export_risk` gates (`block_crc_read_rate`, `attach_stream_crc_events`).
- [x] Freeze matrix: `spec.md` §3.4. Poly-class CRC alone does not elevate post evaluation.
- [x] Split deferred: close export-risk honesty in 0099; park fingerprint as `D-0077-poly-fingerprint`.
- [x] Expand spec; set registry **Ready**.

Do not re-open Phase 0 during coding unless a test proves dual-rate mis-classifies localized corruption as poly. That would be a new defect, not a fingerprint track sneak-in.

---

## Phase 1 — Helper + `compute_export_risk` → DoD-1, DoD-2, DoD-3

- [x] Add `CrcSourceClass`, `PolyCrcRiskAdjustment`, `poly_crc_risk_adjustment` in `crates/pst-dedup-cli/src/unique_export_report.rs`. Use `saturating_add` on CRC/read sums (`spec.md` §3.1).
- [x] Add `crc_source_classes_from_files` (1:1 field map; no pre-averaged rates). Unit-test poly+localized files → two classes with raw counters.
- [x] Extend `ExportRiskInputs` with additive `#[serde(default)]` fields (`spec.md` §3.2). **`impl Default`** so tests can `..Default::default()`. Update the six existing literals (4 tests + 2 call sites).
- [x] Thresholds use `effective_block_crc_read_rate` when `Some`; skip attach-CRC advisory when `discount_attach_stream_crc`; never discount fail-rate / volume / scan.
- [x] When poly-discounted, emit `poly_class_crc_discounted`; do **not** emit raw `block_crc_read_rate=1.000>0.15`.
- [x] Unit tests (same file as existing export_risk tests). Prefer a **table-driven** `#[test]` over spec §3.4 rows (job shape → level + required reasons), plus:

| Test | Expect |
|---|---|
| `poly_crc_all_poly_zero_effective` | two dual-rate sources → effective `Some(0.0)`, discounted true, discount attach true |
| `poly_crc_localized_only_passthrough` | high block, low page → effective ≈ raw, discounted false, discount attach false |
| `poly_crc_mixed_poly_plus_localized` | effective = localized rate; discount attach **false**; `non_poly_crc_noisy_sources=1` |
| `poly_crc_mixed_poly_plus_clean` | effective ≈ 0; discount attach true |
| `poly_crc_empty_fail_closed` | `effective=None`, flags false |
| `export_risk_all_poly_inc_like_ok` | raw 1.0 + 6014 events + scan ok → **ok** + `poly_class_crc_discounted` |
| `export_risk_catastrophic_read_rate_without_failed_volume` | **keep** — no poly flags → still `not_export_ready` |
| `export_risk_attach_stream_crc_events_recommend_reexport` | **keep** — 1 event, no discount → `re_export_recommended` |
| `export_risk_poly_does_not_lower_scan_not_export_ready` | scan NER + all-poly discount → still NER |
| `export_risk_poly_plus_attach_fail_still_advisory` | discount CRC; `attach_fail_rate=0.06` → `re_export_recommended` |
| `export_risk_mixed_localized_still_catastrophic` | effective 0.20, no attach discount → NER (also a §3.4 table row) |
| `crc_source_classes_from_files_maps_raw_counters` | poly + localized `FileScanStats` → two classes; rates not pre-averaged |
| `export_risk_matrix_table_driven` | spec §3.4 rows → level + reasons (incl. co-occurring `poly_class_crc_discounted` + scan NER) |

- [x] `cargo test -p pst-dedup-cli --lib` and `cargo test -p dedup-engine` for the above. No `unwrap`/`expect` in production paths.

## Phase 2 — unique-pst wire-up → DoD-1, DoD-3

- [x] Call `crc_source_classes_from_files(&summary.files)` then `poly_crc_risk_adjustment` at success-path `compute_export_risk` (~3054).
- [x] Pass raw + adjustment fields. Cancel-path constructor (~1125) stays undiscounted zeros.
- [x] Comment at the call site: if `--jobs` ever omits per-source CRC (`D-0077-parallel-attrib`), skip adjustment (fail closed).
- [x] Extend `export_oracle.rs` `compare_integrity_counters` with the four `/export_risk/inputs/…` pointers (`spec.md` §3.3). Do **not** allowlist them.
- [x] Human summary: print `export_risk` level as today; if discounted, the JSON reasons carry the attest. Optional one-line `poly_class_crc_discounted` next to `export_risk` if a CRC line already exists — do not invent a subject/filename log line (0077 rule 7).
- [x] GUI: no new widgets. Banner already follows `level`. Confirm `unique_worker` / wizard still compile (`cargo check -p pst-dedup-gui`).
- [x] Do **not** change `--fail-on-export-risk` parse or 0078 exit integers. Touch `export_exit_0078.rs` / unique-pst integration tests **only** if they assert `not_export_ready` from synthetic poly-class rates. Do not weaken localized-CRC cases.

## Phase 3 — Docs + deferred → DoD-4

- [x] `docs/unique-pst-export.md` CRC table: poly-class exception; effective vs raw; inexact bids expected on poly sources; `ATTACH_STREAM_CRC` on poly-only jobs does not elevate; **`poly_class_crc_discounted` may co-occur** with a non-CRC `not_export_ready` reason.
- [x] `docs/unique-pst-ediscovery-runbook.md` integrity table: same; vocabulary still frozen; 0.15 remains the catastrophic constant on the **effective** rate.
- [x] `docs/deferred.md`: close export-risk half of `D-0077-systematic-poly`; keep `D-0077-poly-fingerprint`; record **`D-0099-attach-crc-job-level`**; update `D-0094-inc-resmoke` when 0099 Completes.
- [x] CHANGELOG Unreleased: 0099 honesty line (implementation, not this planning commit).

## Phase 4 — Finalize → DoD-5

- [x] `review.md` (commands, tests, optional INC* smoke counts only — no subjects).
- [x] Registry **Completed**; `sequencing.md` / `ROADMAP.md` / `conductor.md`.
- [x] Ledger commit implementation tx. Optional operator re-smoke INC* → `output/inc0102784-post-0099/` (gitignored).

---

## Handoff notes

- Never mutate operator source PSTs.
- INC* evidence stays operator-local (`output/`, Desktop).
- Do not steal IDs **0100** / **0101**; frontend Series O stays **0105+**.
- Do not implement polynomial fingerprint or per-event attach-CRC split “while you’re here.”
- 0098 is on `origin/main` (`20f7aae`). Do not fold 0099 implementation into that commit.
- Hygiene: untracked root `agy-review.md` / `fixtures/keep_set_summary.json` — do **not** commit. Track-local `opencode-review.md` + `agy-review.md` stay with the track (force-add when 0099 lands).
