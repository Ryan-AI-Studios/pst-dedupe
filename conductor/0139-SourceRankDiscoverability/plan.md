# 0139 — SourceRankDiscoverability — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`. Status stays **Ready — not started** until implement.
>
> **Ledger (execute):** `ledgerful ledger start 0139-source-rank-discoverability --category FEATURE --message "CLI help + JSON input_path_sort_order + multi-input source-rank note"`
>
> **Fold-in 2026-09-25:** `agy-review.md` + `opencode-review.md` (bundle `AI-review.md`). Placeholder expanded; oracle root-remove; warning predicate is `--source-rank` only.

---

## Phase 0 — Re-verify → DoD-5 (precondition)

- [ ] Re-read `sort_input_paths`, `KeepPolicy::FirstSeen`, `source_rank_of`, `prefer_path_policy_key`, `rank_key` in `crates/dedup-engine/src/keepset.rs`.
- [ ] Confirm keep-set / unique-pst / unique-eml still `sort_input_paths` before `run_scan`.
- [ ] Confirm `export_oracle.rs` still blanks root `inputs` and that `"inputs"` is **not** on `SUMMARY_ALLOWLIST_KEYS`.
- [ ] Confirm unique-pst `--also-eml` calls `write_eml_pack_from_keep_set` and **not** `run_unique_eml`.
- [ ] Confirm `--policy` help already has the first_seen sentence (`main.rs` keep-set + unique-eml; `unique_pst_cmd.rs` UniquePst CLI).
- [ ] Do **not** change ranking. Do **not** default-on `--source-rank`. Do **not** emit from `scan` / `dups`.

---

## Phase 1 — Helper + unit tests → DoD-3, DoD-5

- [ ] Add `pub fn multi_input_source_rank_hint(input_count: usize, source_rank_patterns: &[String]) -> Option<String>` next to `recoverable_items_hint`.
- [ ] Re-export from `crates/dedup-engine/src/lib.rs`.
- [ ] Unit tests: 1 input → `None`; 2 inputs empty rank → `Some` with pinned substrings; non-empty `--source-rank` → `None`; empty-string-only patterns → treat as no rank (`Some`).
- [ ] Optional rustdoc on `KeepPolicy::FirstSeen` / `sort_input_paths`: ASCII `'-'` (`0x2D`) before `'.'` (`0x2E`) so `foo-2.pst` sorts before `foo.pst` on Windows lowercased paths. Comment only; no behavior change.

---

## Phase 2 — Help + JSON + emit → DoD-1, DoD-2, DoD-3

- [ ] Extend `--source-rank` `///` on keep-set, unique-eml (`main.rs`) and unique-pst (`unique_pst_cmd.rs`): ordered best-first; default `first_seen` uses sorted resolved paths; `-2.pst` can crown before `.pst`; pass `--source-rank` to override. Do **not** copy the `--policy` sentence.
- [ ] Add `input_path_sort_order: Vec<String>` to `KeepSetSummaryOut`, `UniqueExportSummary`, `UniqueEmlSummaryOut`. Fill from post-sort `paths` (display strings). unique-pst keep `inputs` and set both from the same iterator.
- [ ] Fill UniqueEmlSummaryOut at **both** construction sites.
- [ ] After `sort_input_paths`, if helper returns `Some`, emit once:
  - keep-set / unique-eml: `eprintln!("note: {hint}")` even when `--json`
  - unique-pst: `emit_log(..., "note: {hint}")`
- [ ] Do **not** emit inside `write_eml_pack_from_keep_set_inner` (also-eml stays one line).
- [ ] Thin export.md sentence that the CLI prints this run-level note. Runbook optional one clause; do not rewrite 0131 RI copy.

---

## Phase 3 — Oracle → DoD-4

- [ ] In `normalize_summary_for_oracle`, same root-object block as `inputs` blanking: `obj.remove("input_path_sort_order")`.
- [ ] Comment: path-local; parent packs omit the key; **not** on `SUMMARY_ALLOWLIST_KEYS` (D-0099 name-based strip).
- [ ] Unit test: parent JSON without the key vs HEAD with `input_path_sort_order: ["C:/tmp/a-2.pst", "C:/tmp/a.pst"]` (+ existing `inputs`) are equal after normalize.
- [ ] Assert `"input_path_sort_order"` is absent from `SUMMARY_ALLOWLIST_KEYS` (same style as the 0108 allowlist-negative tests).

---

## Phase 4 — CLI tests → DoD-1, DoD-2, DoD-3, DoD-5

Prefer extending `crates/pst-dedup-cli/tests/keep_set.rs` (already copies `a.pst` / `a-2.pst`) over a new unique-pst write.

- [ ] Two inputs, no rank, `--json`: stderr contains helper substrings **once**; stdout parses; `input_path_sort_order[0]` basename is `a-2.pst`; array length 2; `exit_code` still success.
- [ ] Same with `--source-rank a.pst --source-rank a-2.pst`: **zero** helper matches; JSON field still present (order used).
- [ ] Same with `--prefer-path-contains Inbox` only: **one** helper match (predicate ignores prefer-path).
- [ ] Help tests: keep-set / unique-pst / unique-eml `--help` contains path-sort / `-2` / `source-rank` language beyond the bare flag token.
- [ ] Optional: `scan` two PSTs `--json` stderr does not contain `without --source-rank`.
- [ ] If a cheap unique-pst `--also-eml` fixture already exists in `unique_pst_also_eml.rs` / depth tests, add `matches(…).count() == 1` for the new note. If that path is too heavy, document in `review.md` that also-eml silence is guaranteed by not calling the helper from the pack inner, plus unique-eml standalone coverage.

---

## Phase N — Finalize → DoD-6

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Targeted tests in spec §8, then `cargo test --workspace`
- [ ] `ledgerful verify`
- [ ] CHANGELOG user-facing CLI
- [ ] `review.md`; registry **Completed**; ledger commit **FEATURE**
- [ ] Optional owner HITL on Desktop INC* pair — skip allowed if recorded. Never `git add` those PSTs.

---

## Handoff notes

- Ranking is frozen. If a test wants `a-2.pst` to lose, it must pass `--source-rank`.
- unique-pst is a hotspot: help + one emit after sort + one summary field + oracle. No writer/materialize edits.
- 0141/0145 consume the wrapper field; they are not this track.
- Single-exe / no-daemon unchanged.
