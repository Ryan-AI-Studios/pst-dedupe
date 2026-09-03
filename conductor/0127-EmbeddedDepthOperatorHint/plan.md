# 0127 — EmbeddedDepthOperatorHint — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Ledger (execute):** after owner git-commits Ready docs: `ledgerful ledger start 0127-embedded-depth-hint --category FEATURE --message "ATTACH_DEPTH_LIMIT stderr names --max-embedded-depth cap"`
>
> **Fold-in 2026-09-03:** opencode-M1/m1 + AGY-127-01..04.

## Phase 0 → DoD-4

- [ ] Re-read clap default 3 (`unique_pst_cmd.rs` **and** `main.rs` UniqueEml); `unique_pst_depth.rs` default-3 test; `unique_eml_depth.rs`. Confirm default still 3.
- [ ] Confirm Off ledger does not populate `attachments_failed_by_reason` / `failed_by_reason`. Plan the count from materializer `embedded_extract_limit` / writer `ATTACH_DEPTH_LIMIT`.
- [ ] Do **not** edit matter-core schema. Do **not** close D-0067.

## Phase 1 → DoD-1, DoD-2, DoD-3

- [ ] unique-pst: hint via `emit_log` when depth-limit count > 0 (not the Option histogram). No new output channel.
- [ ] unique-eml: `writeln!` / `eprintln!` to stderr when count > 0, **before** `Ok(classified_exit)` / `--json` stdout — including `--allow-partial-fidelity` success. Do not gate on `classified_exit != Success`.
- [ ] clap help sentence in **both** `unique_pst_cmd.rs` and `main.rs`. Keep `default_value_t = 3`.
- [ ] Runbook 2026-09-02 + footgun wording.
- [ ] Tests: `unique_pst_depth` default-3 stderr contains `--max-embedded-depth=3`; `ceiling_8_fails_at_7_succeeds_at_8` stderr contains `--max-embedded-depth=7`; `unique_eml_depth.rs` default-3 (and depth-7 if present) `assert!(stderr.contains("--max-embedded-depth="))`.

## Phase 2 → DoD-4, DoD-5

- [ ] `cargo test -p pst-dedup-cli --test unique_pst_depth --test unique_eml_depth`
- [ ] fmt / clippy / CHANGELOG / `review.md` / registry Completed / ledger commit
