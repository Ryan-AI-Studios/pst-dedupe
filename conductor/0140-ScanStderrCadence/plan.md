# 0140 — ScanStderrCadence — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`. Status stays **Ready — not started** until implement.
>
> **Ledger (execute):** `ledgerful ledger start 0140-scan-stderr-cadence --category FEATURE --message "Periodic -v scan progress; crc-log-limit 0 silences deep-attach attempted= lines"`
>
> **Fold-in 2026-09-25:** `agy-review.md` + `opencode-review.md` (bundle `AI-review.md`). Hoist `first_n`; re-arm folder timer; unique-pst summary retained; aspose CLI; `scan_integrity` verify target.

---

## Phase 0 — Re-verify → DoD-5 (precondition)

- [ ] Re-read `scan.rs` folder loop (`tracing::info` `"scan progress"` + `PROGRESS_EVERY_MSGS`).
- [ ] Re-read `init_tracing` (`warn` / `info` / `debug`) and global `-v` clap.
- [ ] Re-read probe `progress_cb` in `scan.rs` and `unique_pst_cmd.rs` (500-filter + writeln).
- [ ] Re-read `set_log_limit` / `maybe_emit` (`first_n = 0` totals-only). Confirm no getter yet.
- [ ] Confirm unique-pst `stage=` / `emit_stage_progress` is **out of scope**.
- [ ] Confirm `--crc-log-limit` exists on scan, dups, keep-set, unique-pst, unique-eml.

---

## Phase 1 — Helpers + unit tests → DoD-1, DoD-3, DoD-4

- [ ] Add `pst_reader::integrity_telemetry::log_first_n() -> u64` (read `LOG_CONFIG.first_n`; on lock failure use the same default as `read_log_config`).
- [ ] Getter unit **inside** `integrity_telemetry.rs` tests: wrap with existing `with_lock` / `TEST_LOCK`; `set_log_limit(0, …)` then `log_first_n() == 0`; `reset()` / restore `set_log_limit(10, …)` in teardown. Do not race `bounded_emission_with_exact_total`.
- [ ] Add `crates/pst-dedup-cli/src/scan_progress.rs` (re-export inside `pst_dedup_cli`). Both helpers and their unit tests live here — not in `pst-reader`, not inlined into hotspot files.
- [ ] `should_emit_folder_progress(folder_i_1based, folder_count, elapsed_since_last, every_folders: 250, every: 2s) -> bool`. True on first, last, every 250, or elapsed ≥ 2 s.
- [ ] `should_emit_probe_progress_line(attempted, first_n) -> bool`: `first_n == 0` ⇒ false; else `attempted == 1 || attempted.is_multiple_of(500)`.
- [ ] Unit tests: 40 folders → emit count `< 40` and includes 1 and 40; 250th emits; 2 s elapsed emits then **caller must re-arm** (test the helper with a second call using elapsed=0 after a true, expect false unless first/last/250); `first_n=0` never emits probe lines; `first_n=10` emits 1, 500, 1000.

---

## Phase 2 — Wire emit sites → DoD-1, DoD-2, DoD-3, DoD-4

- [ ] Folder loop **per file**: `folder_i = 0`; `last_folder_emit = Instant::now()` at file start. For each folder: increment `folder_i`; `elapsed = last_folder_emit.elapsed()`; if `should_emit_folder_progress` { `tracing::info!(file, folder, recoverable, skipped, "scan progress")`; `last_folder_emit = Instant::now();` } else { `tracing::debug!(file, folder, recoverable, skipped, "scan progress")`; }. **Omit `msg_i`** on these events. Keep message-every-500 INFO **with** `msg_i`.
- [ ] `scan.rs` probe: `let first_n = pst_reader::integrity_telemetry::log_first_n();` **once** before `progress_cb`. Closure: `if should_emit_probe_progress_line(attempted, first_n) { writeln!(…) }`. No getter inside the loop.
- [ ] unique-pst probe: after `apply_crc_log_limits`, `let first_n = args.crc_log_limit;` once; same helper in `progress_cb` (gates stderr **and** `on_log`). **Leave** the end-of-probe `emit_log` summary (~1973) unchanged.
- [ ] unique-pst authorized extra edit: extend `UniquePstClapArgs::crc_log_limit` `///` (`unique_pst_cmd.rs` ~238): `0` = CRC totals-only **and** no per-attempt deep-attach `attempted=` progress lines (end-of-probe summary still prints). No other unique-pst edits.
- [ ] Same `--crc-log-limit` `///` on scan / dups / keep-set / unique-eml in `main.rs`.
- [ ] Global verbose `///`: `-v` periodic **scan/dups/keep-set** folder progress; `-vv` one line per folder. Do not claim unique-pst `stage=` is cadenced here.
- [ ] CHANGELOG Unreleased. One sentence in `docs/unique-pst-export.md`: 0065 §3.11 per-folder `-v` superseded; unique-pst keeps one attach-preflight summary; `--crc-log-limit 0` mutes per-attempt probe ticks (including GUI `on_log`).

---

## Phase 3 — CLI tests → DoD-1, DoD-4, DoD-5

Use `fixtures/aspose_outlook.pst` (already in keep-set / 0077 / 0078 tests). Re-count folders at execute (review claimed 27). Count **folder-progress** lines as stderr containing `folder=` and **not** `msg_i=`. Per-attempt probe lines match `attempted=` **and** `bytes=` **and** `source=` (excludes unique-pst summary).

- [ ] Default `scan --json` (no `-v`): zero folder-progress lines; stdout parses.
- [ ] `scan -v --json`: folder-progress count **strictly less** than JSON/file folder count (expect first+last only if folders < 250 and wall < 2 s).
- [ ] `scan -vv --json`: one folder-progress line per folder (info+debug together = folder count, not 2× cadence folders).
- [ ] Helper 40-tick test remains the DoD-1 **N ≥ 40** proof.
- [ ] `scan --deep-attach-preflight --crc-log-limit 0 --json`: zero per-attempt probe lines; JSON `attach_probe` shows the probe ran (`enabled` / `attempted` as live schema). If this fixture’s probe is `enabled: false` with 0 attempts, still assert zero per-attempt lines and cover the helper + both `progress_cb` call sites.
- [ ] `--json` stdout: one JSON value.

Do **not** git-add INC* PSTs. Optional HITL in review.md.

---

## Phase N — Finalize → DoD-6

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Targeted tests in spec §8, then `cargo test --workspace`
- [ ] `ledgerful verify`
- [ ] `review.md`; registry **Completed**; ledger commit **FEATURE**
- [ ] Optional owner HITL on Desktop INC* pair — skip allowed if recorded.

---

## Handoff notes

- 0065 §3.11 “per folder if `-v`” is **replaced** by this cadence. Do not restore 1:1 folder INFO at `-v`.
- unique-pst `stage=` remains 0132. Per-attempt probe `attempted=` is this track; the one end-of-probe summary stays.
- Gating `progress_cb` also silences GUI `on_log` probe ticks at `--crc-log-limit 0` (`D-0074-gui` stays residual).
- **0143** owns scary CRC *copy*; this track only bounds *volume*.
- **0147** owns truncated-budget JSON leftover; this track only probe **lines**.
