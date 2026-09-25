# 0143 — PolyCrcScanCopy — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`. Status: **Completed** (PR **#162** / `1617720`).
>
> **Ledger (execute):** `ledgerful ledger start 0143-poly-crc-scan-copy --category FEATURE --message "poly-class CRC operator copy on scan JSON/stderr; preflight math unchanged"`
>
> **Fold-in 2026-09-25:** `agy-review.md` + `opencode-review.md` (bundle `AI-review.md`). Additive `ScanSummary.poly_crc_note` + stderr `note:`; never `preflight.reasons`; aspose is the CI poly fixture.

---

## Phase 0 — Re-verify → DoD-3 (precondition)

- [ ] Re-read `compute_preflight` (`integrity.rs` ~611–702): empty `reasons` in the skip branch stays `Ok`; non-empty → `ReExportRecommended`. Confirm it still ignores `block_crc_read_rate`.
- [ ] Re-read `is_poly_class_crc` + `clear_poly_false_positive_crc_suspect` (`scan.rs` ~1263–1302, ~1844–1853). Do not edit.
- [ ] Confirm `run_scan` has no `eprintln!` / `println!`.
- [ ] Confirm 0140 `--crc-log-limit` help tests (`scan_integrity`, `dups`, `keep_set`, unique-*).
- [ ] Re-run `scan --json` on `fixtures/aspose_outlook.pst`: expect `poly_class_crc_sources >= 1`, `files[].poly_class_crc == true`, `preflight.recommendation == ok`. Fold-in saw sources=1, read_rate=1.0, reasons=[].
- [ ] List every `ScanSummary {` literal (spec §2.2: 11 + constructor).

---

## Phase 1 — Helper + JSON + literals → DoD-1, DoD-3, DoD-4, DoD-5

- [ ] Add `poly_crc_note(sources: u64) -> Option<String>` in `scan.rs` with the frozen sentence in spec §2.5. Unit-test 0 → `None`; 1 and 2 → `Some` with `{n} source(s)`.
- [ ] Add `poly_crc_note: Option<String>` to `ScanSummary` with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- [ ] Set it in the `run_scan` constructor from `poly_class_crc_sources`.
- [ ] Update **all** other `ScanSummary {` literals to `poly_crc_note: None` (cancelled unique-pst, unique-eml dummies, `scan.rs` exit-policy tests, `unique_pst_also_eml.rs` ×6).
- [ ] Do **not** edit `PreflightReport`, `compute_preflight`, dual-rate, or `dedup-engine` besides leaving it alone.
- [ ] Add `"poly_crc_note"` to `SUMMARY_ALLOWLIST_KEYS`. Test: parent JSON without the key equals HEAD with a note after `normalize_summary_for_oracle`.

---

## Phase 2 — Stderr + human → DoD-2, DoD-5

- [ ] `eprint_poly_crc_note(sources)` or print `note: {note}` when `Some`. Call **after** `run_scan` in `cmd_scan`, `cmd_dups`, `keep_set_cmd`, `unique_pst_cmd`, `unique_eml_cmd`.
- [ ] keep-set / unique-eml: poly line **after** 0139 source-rank note (source-rank is already before scan).
- [ ] Independent of `set_log_limit` / `--crc-log-limit`. Use `eprintln!`, not `tracing`.
- [ ] `print_summary_text`: append `poly_sources={}` on the `crc:` line; if note `Some`, print `  poly_crc:     {note}` after `preflight:`.

---

## Phase 3 — Docs → DoD-6

- [ ] README: poly-class CRC can coexist with preflight `ok`; see `poly_crc_note` / `poly_class_crc_sources`. `block_crc_read_rate` is the fraction; `block_crc_rate` is hits per message.
- [ ] CHANGELOG Unreleased: Changed (0143).
- [ ] Close `D-0143-poly-crc-scan-copy` on the implement **docs** PR (not this fold).

---

## Phase 4 — Tests → DoD-1 … DoD-5

Use `fixtures/aspose_outlook.pst`. No `include_str!`. Do not git-add INC* PSTs. Prefer extend `tests/scan_integrity.rs` **or** add `tests/poly_crc_copy.rs` (implementer choice; if no dedicated file, skip that `--test` in §8).

- [ ] Unit: `poly_crc_note(0/1/2)` as Phase 1.
- [ ] `scan --json` aspose: `poly_class_crc` true; `poly_class_crc_sources >= 1`; `poly_crc_note` equals helper(count); `preflight.recommendation == "ok"`; `reasons` has no poly token; stderr contains `note:` and `poly-class CRC`; stdout JSON parses (no `note:` as the JSON document).
- [ ] Same with `--crc-log-limit 0`: poly `note:` still present; 0140 probe silence still holds if that test is co-run.
- [ ] `dups --json` aspose: `summary.poly_crc_note` present.
- [ ] Human `scan` aspose (no `--json`): stdout contains `poly_sources=` and `poly_crc:`.
- [ ] Serialize `ScanSummary` with `poly_class_crc_sources: 0` / `poly_crc_note: None`: key **absent**.
- [ ] Deserialize pre-0143 JSON (no `poly_crc_note`) into `ScanSummary` succeeds (`serde default`).
- [ ] Existing `compute_preflight` / `crc_integrity_0077` / 0140 crc-log-limit help tests still pass.

Optional: keep-set aspose `--json` stderr has the poly `note:`.

---

## Phase N — Finalize → DoD-6

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Targeted tests in spec §8, then `cargo test --workspace`
- [ ] `ledgerful verify`
- [ ] `review.md`; registry **Completed**; ledger commit **FEATURE**
- [ ] Optional owner HITL on Desktop INC* `scan --json` — skip allowed if recorded.

---

## Handoff notes

- Frozen note talks about compatibility with preflight `'ok'`; it must not interpolate this run’s recommendation (poly + skip_rate can still be `re_export_recommended` for **other** reasons).
- Do not print `block_crc_rate` as a percent. INC* ~30 is hits per message; aspose fold-in was ~14.
- **0144** owns header-only integrity CSV. Do not add CLASS rows here.
- GUI `worker::run_scan` is a different function — leave it.
