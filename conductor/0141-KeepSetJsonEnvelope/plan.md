# 0141 — KeepSetJsonEnvelope — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`. Status: **Completed** (PR **#158** / `766f523`).
>
> **Ledger (execute):** `ledgerful ledger start 0141-keepset-json-envelope --category FEATURE --message "keep-set --json agent envelope; winners in sidecar or --include-winners"`
>
> **Fold-in 2026-09-25:** `agy-review.md` + `opencode-review.md` (bundle `AI-review.md`). Default-omit winners; envelope schema `keep_set_summary_v1`; disk always envelope; CLI-private type; live test inventory.

---

## Phase 0 — Re-verify → DoD-5 (precondition)

- [ ] Re-read `KeepSetSummaryOut` + `build_summary` (both call sites ~358 and ~386) + stdout `println` + `AlreadyEmitted`.
- [ ] Re-read `KeepSet` in `dedup-engine` (`winners` required `Vec`). Confirm no `include_str!` keep-set JSON tests — parsers are `serde_json::from_str` on captured stdout.
- [ ] Confirm unique-pst / unique-eml read `--keep-set-json` / report files, not keep-set stdout.
- [ ] Confirm `export_oracle.rs` `/keep_set/winners` is unique-export summary — **out of scope**.
- [ ] List clap keep-set flags in `main.rs`; 0139 `input_path_sort_order`; 0140 `--crc-log-limit` `///`.

---

## Phase 1 — Envelope type + flag → DoD-1, DoD-2, DoD-4

- [ ] Add `KeepSetCliArgs.include_winners: bool`. clap on `Commands::KeepSet`: `#[arg(long = "include-winners")]` with `///` restoring inline `keep_set.winners` under `--json` (no-op without `--json`). Thread in `main.rs` next to `json`.
- [ ] CLI-private nested keep-set view in `keep_set_cmd.rs`: all current `KeepSet` JSON fields except `winners` is `Option<Vec<KeepEntry>>` + `skip_serializing_if = "Option::is_none"`. Do **not** edit `dedup_engine::KeepSet`.
- [ ] Top-level `schema`: `"keep_set_summary_v1"`. Add `winners_inline: bool`. Keep every other `KeepSetSummaryOut` field including `input_path_sort_order`.
- [ ] `build_summary(..., inline_winners: bool)`:
  - disk / default: `inline_winners = false` → nested `winners` omitted, `winners_inline = false`
  - stdout with `--json --include-winners`: rebuild with `inline_winners = true`
- [ ] Write-failure rewrite (~386) still uses **disk** envelope (`inline_winners = false`).
- [ ] `--include-winners` without `--json`: skip rebuild; human summary unchanged.

---

## Phase 2 — Docs + help → DoD-2, DoD-6

- [ ] `keep-set --help`: `--include-winners` present; `--json` / `--keep-set-json` text: stdout is envelope; sidecar is winners source.
- [ ] README keep-set block (~76–86): `--json` is stats envelope; winners live in `--keep-set-json`; `--include-winners` restores stdout winners.
- [ ] CHANGELOG Unreleased: Changed (0141).
- [ ] One sentence in `docs/unique-eml-import.md` (or keep-set runbook if that is where `--json` is described): sidecar is the winners document.

---

## Phase 3 — Tests → DoD-1, DoD-2, DoD-3, DoD-4

Use `fixtures/aspose_outlook.pst`. No `include_str!`. Do not git-add INC* PSTs.

**Migrate existing stdout winner parsers** (live names):

| Test | Change |
|---|---|
| `keep_set_json_schema_and_decision_csv_header` | stdout `schema == keep_set_summary_v1`; `winners_inline == false`; `keep_set.winners` **not** an array (null/absent); sidecar `ks_v["schema"] == keep_set_v1` **and** `ks_v["winners"]` is array |
| `keep_set_input_flag_works` | stdout schema `keep_set_summary_v1` (today `keep_set_v1`) |
| `path_order_determinism_two_copies` | add `--include-winners` so ranking still reads stdout winners |
| `source_rank_flips_winner_file_a_vs_a2` | add `--include-winners` |
| `aspose_default_winners_deterministic_golden` | golden keys from **sidecar** `winners`; stdout omits winners; keep sidecar cross-run match |
| `keep_set_help_lists_0075_flags` | also require `--include-winners` |
| `export_exit_0078` keep-set tests | keep `exit_code` / `fidelity` / `summary_path`; assert disk summary omits `keep_set.winners` |

**New tests in `keep_set.rs`:**

- [ ] Default `--json`: stdout bytes **< 16 KiB**; `winners` key absent; `winners_inline == false`; `keep_set.stats.unique` > 0.
- [ ] `--json --include-winners --keep-set-json <path>`: stdout `winners` equals sidecar `winners`; `winners_inline == true`.
- [ ] `--json --include-winners`: `keep_set_summary.json` on disk still omits `winners` and has `winners_inline == false`.

unique-eml / unique-pst sidecar tests: **do not change** unless they break (they should not).

---

## Phase N — Finalize → DoD-6

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Targeted tests in spec §8, then `cargo test --workspace`
- [ ] `ledgerful verify`
- [ ] `review.md`; registry **Completed**; ledger commit **FEATURE**
- [ ] Optional owner HITL on Desktop INC* `keep-set --json` stdout size — skip allowed if recorded.

---

## Handoff notes

- Envelope schema **`keep_set_summary_v1`** is CLI stdout/summary, not matter schema 41.
- Sidecar **`keep_set_v1`** is the winners source for unique-eml / unique-pst / operators.
- unique-pst JSON `keep_set.winners` stays full — **0141 does not shrink export summaries**.
- **0145** may later consume scan JSON; do not invent `--from-scan-json` here.
