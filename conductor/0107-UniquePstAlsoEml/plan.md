# 0107 — Unique-PST `--also-eml` Co-Export — Plan

> Phased checklist mapped to `spec.md` §7. Planning-only Phase 0 is **closed**. Do not implement until the user says Implement.
>
> **Ledger (implement):** `ledgerful ledger start crates/pst-dedup-cli --category FEATURE --message "0107 unique-pst --also-eml co-export from same keep-set"`
>
> **Fold-in (2026-08-29):** `opencode-review.md` + `agy-review.md` → spec §2.9 / `foldin-note.md`. Lock: real `ScanSummary`+`scan_ok`; `soft_skip_attach_records`; 0078 precedence `130 > 1 > 65 > 64 > 0`; copy `method5_chain`; cancel-during-PST-write skips also-eml; no `skip_serializing_if`; volume-sibling guard.
>
> Re-verify at execute: `run_unique_pst_with_options` still warns-and-ignores `also_eml` at ~1388; `prepare_out_dir` still private in `unique_eml_cmd.rs`; `log_and_progress_callbacks_fire` still asserts the unimplemented warning (`main` @ plan-time `65112ec`).

---

## Phase 0 — Spec expand → Ready (closed 2026-08-28)

- [x] Live `--also-eml` resolves the path then warns “accepted but not implemented (D-0071-also-eml residual); ignoring” (`unique_pst_cmd.rs` ~1388 @ `65112ec`).
- [x] unique-eml 0106 nested MIME shipped (PR #102 / `55de823`); standalone `run_unique_eml` still scans + keep-set.
- [x] Locked fix: extract write-from-keep-set helper; unique-pst calls it with the **same** `KeepSet` after PST write; no `run_unique_eml`; combined 0078 precedence exit; summary always-present `also_eml_*` keys; oracle strips `also_eml_out` only.
- [x] Deferred §9; last-PR comments (#103–#100 none). Frontend **0108+**.
- [x] Status **Ready — not started**.
- [x] RFC 2046 / RFC 5322: reuse 0106 writer (no MIME fork). MS-PST N/A.
- [x] Fold-in: M1–M2 / m1–m5 applied; raw `max(u8)` declined; `UniqueEmlClapArgs` not in scope.

---

## Phase 1 — Shared write-from-keep-set helper → DoD-1

Files: `crates/pst-dedup-cli/src/unique_eml_cmd.rs` (re-verify line numbers at execute; plan-time `main` @ `65112ec`).

- [ ] Extract the unique-eml write loop after keep-set exists (re-materialize, nested extract, `VolumePackWriter`, `write_canonical_eml`, attach ledger, `{out}/summary.json`, `write_eml_pack_manifest`) into a helper visible to `unique_pst_cmd` (`pub` or `pub(crate)` — same lib crate; `pub(crate)` is enough). Standalone `run_unique_eml` still does scan + resolve + promote finalize `|_msg| Ok(())` then **calls the helper**.
- [ ] Helper inputs (behavioral lock, names implementer-choice): keep-set, input paths, out dir (already prepared), `EmlWriteOpts` (family + depth), ledger mode/max-rows/path-mode, `files_per_volume` + `volume_prefix` (unique-pst passes unique-eml defaults 10000 / `VOL`), materializer + attach stream source, cancel flag, log/progress hooks, **unique-pst `ScanSummary` + `scan_ok` (`evaluate_exit_policy` — never fabricate a clean scan)**, **`&[SoftSkipAttachRecord]` (or `&resolved.soft_skip_attach_records`; `to_keep_set(&self)` leaves `resolved` alive)**, unique-pst `fail_on_partial_fidelity` / `allow_partial_fidelity`. Promoted marks come from `keep_set.winners[].promoted_from_failure`. EML classify: `RiskGate::Off` + `PreflightRecommendation::Ok`. `{also_eml}/summary.json` `scan` field = the passed `ScanSummary`.
- [ ] Before the write loop, drain Mode A honesty like `unique_eml_cmd.rs:314-355`: `mark_promoted_winner` then `enqueue_soft_skip_row` for each soft-skip record. Omitting this is a silent EML-ledger drop.
- [ ] Make `prepare_out_dir` `pub(crate)` (or move next to the helper). unique-pst uses it for the also-eml dir. Do not fork the non-empty / `--overwrite` clear logic.
- [ ] Nested extract on the helper’s write loop: `materialize_nested_for_winner(&mut attach_src, &mut msg, nested_depth)` **before** `write_canonical_eml` (0106). Log extract errors as warnings; missing DTO follows 0106 honesty skip.
- [ ] `parents_only` (`FamilyPolicy::ParentsOnly`) still omits nested/file parts.
- [ ] Do **not** change `eml_pack.rs` MIME policy this track unless a compile break forces a thin glue fix (prefer not).
- [ ] No `unwrap` / `expect` in production.

---

## Phase 2 — unique-pst wire → DoD-1, DoD-2 (flag + summary)

Files: `crates/pst-dedup-cli/src/unique_pst_cmd.rs`, `unique_export_report.rs`, `export_oracle.rs`.

- [ ] **Delete** the warn-and-ignore block at ~1388. Keep `resolve_cli_path_maybe_missing`.
- [ ] Extend path guard so `--also-eml` cannot equal / nest with inputs, `--out`, `--report-dir`, decision/keep-set/integrity artifacts (spec §3.5). Also: no planned PST volume sibling (`volume_path_for(out, 2..=MAX)`) is `is_same_or_under` the also-eml dir. Fail closed **before** scan.
- [ ] After unique-pst keep-set exists and PST write has run: if `also_eml` is `Some` **and `cancelled` is false** (cancel during PST write **skips** also-eml; `also_eml_ran=false`; exit 130 from PST classify):
  1. `prepare_out_dir(also_eml, args.overwrite)?` — actually, prepare **up front** with other path guards so a non-empty dir without `--overwrite` fails before a 275 s PST write. Lock: **prepare also-eml dir before scan** (same moment as unique-pst `--out` / report-dir prep).
  2. After PST volumes: `emit_log` `stage=also_eml`; progress stage `also_eml`.
  3. `let eml_family = effective_family;` `let nested_depth = args.max_embedded_depth.clamp(1, 8);` (same values used for PST nested extract).
  4. Reuse `PstHandleCache` / materializer / `PstAttachStreamSource` when still open; otherwise construct with the same `max_open_psts`.
  5. Call the helper with real `ScanSummary` + `scan_ok`, `soft_skip_attach_records`, unique-pst `fail_on_partial` flags. Map helper errors: cancel during also-eml → quarantine **also-eml dir only** + exit 130 + `also_eml_ran=true`; other hard fail → EML `artifact_state` failed, PST kept, combined exit **1** if EML classified Generic.
- [ ] Combined classify: **0078 integer precedence** over unique-pst `CliExit` and also-eml pack classify (`130 > 1 > 65 > 64 > 0`). **Not** raw `max(u8)`. Merge `exit_reason` worst-first without duplicate codes. `summary.ok` follows combined exit. `{also_eml}/summary.json` keeps unique-eml’s own classify (`scan` = unique-pst scan; EML risk Off/Ok).
- [ ] `UniqueExportSummary` always-present fields (Serialize-only; **no** `skip_serializing_if` on these six — do **not** copy `#[serde(skip_serializing_if = "Option::is_none")]` from `max_volume_bytes` / `decision_csv` / `keep_set_json` / `error`; **no** schema id bump):
  - `also_eml_out: Option<String>`
  - `also_eml_ran: bool`
  - `also_eml_eml_written: u64`
  - `also_eml_attach_parts_failed: u64`
  - `also_eml_embedded_messages_written: u64`
  - `also_eml_exit_code: u8` (`0` when not run)
  When flag absent: `also_eml_out: None`, `also_eml_ran: false`, counts 0, `also_eml_exit_code: 0`.
- [ ] `SUMMARY_ALLOWLIST_KEYS`: add **`"also_eml_out"`** only. Do **not** add `"also_eml"`, `also_eml_ran`, or count keys (0102: recursive strip by name).
- [ ] Clap help on `UniquePstArgs.also_eml`: co-export unique-eml pack from the same keep-set; directory required; `--overwrite` replaces non-empty; nested MIME follows `--max-embedded-depth`.
- [ ] `UniquePstCliArgs` comment: drop “soft residual”.
- [ ] Human stderr: print also-eml path + eml/attach counts. No unimplemented warning.
- [ ] GUI wizard stays `also_eml: None` (no checkbox).
- [ ] Tests that construct `UniquePstCliArgs` must compile (new summary fields are on the report struct, not the args struct). `also_eml: None` literals stay valid.

---

## Phase 3 — Tests + docs → DoD-2, DoD-3

- [ ] Invert `log_and_progress_callbacks_fire` also-eml warning assertion (**must fail on HEAD** before invert). Assert pack files exist under `also_eml_unused` (parents-only). Prefer asserting `stage=also_eml` or `done` rather than the old warning.
- [ ] Add `crates/pst-dedup-cli/tests/unique_pst_also_eml.rs` (locked name) covering spec §10.2: aspose count match; overlap usage error; non-empty without overwrite; flag absent JSON **null** `also_eml_out`; method-5 chain inner Subject (**copy** `method5_chain`, no `--no-attachments`); Mode A soft-skip row on the EML ledger.
- [ ] Keep green: `unique_eml`, `unique_eml_depth`, `unique_pst_depth`, export_oracle, `export_exit_0078` (`also_eml: None` unless a test explicitly covers combined 64).
- [ ] Docs: `docs/unique-pst-export.md` flag row (replace “soft residual”); `docs/unique-eml-import.md` co-export sentence; CHANGELOG Unreleased; `docs/deferred.md` **close** `D-0071-also-eml`.
- [ ] Do **not** close `D-0067-embedded-depth`.

---

## Phase 4 — Finalize → DoD-4

- [ ] Write `review.md` in this track dir: results, evidence, and any explicitly-deferred items.
- [ ] Update `../conductor.md`: set this track's status to **Completed**.
- [ ] Commit the ledger transaction in the execution repo.
- [ ] Notify: frontend if started uses **0108+**; `D-0067` still open (matter children); `D-0094-inc-resmoke` still operator HITL.

---

## Handoff notes

- Do not implement until the user says Implement.
- Single-exe / no-daemon constraint unchanged.
- BCC split is intentional: PST default suppress vs EML `Bcc:` from `display_bcc`.
- Cancel during also-eml must not quarantine the unique-PST. Cancel **during PST write** skips also-eml (`also_eml_ran=false`).
- Prepare the also-eml directory **before** scan so `--overwrite` failures are cheap.
- `pub(crate)` is valid here (helper lives in the same lib crate as unique-pst). Unlike 0106’s clap parser, this is **not** crossing into bin `main.rs`.
- Combined exit is 0078 precedence (`130 > 1 > 65 > 64 > 0`), not raw `max(u8)`.
- Re-verify at execute: warn-and-ignore still present; callback test still expects it.
