# 0107 — Unique-PST `--also-eml` Co-Export

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-eml nested MIME (0106),
> identity-hash depth, BCC default, HNBITMAPHDR, matter child-document extract,
> or frontend during implementation.

- **Track ID:** 0107-UniquePstAlsoEml
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `docs/unique-pst-export.md` + `docs/unique-eml-import.md` + this track. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-28); do **not** chase it at execute.
- **Cross-repo contract:** n/a
- **Status:** Ready — not started
- **Depends on:** 0071 (Completed — flag accepted, ignored) · 0067 (Completed — unique-eml pack) · 0089 (Completed — unique-eml attach ledger) · 0106 (Completed — unique-eml nested MIME + `--max-embedded-depth`). Series Q **0105–0106** Completed is the honesty unblock, not a code import.
- **Spec authored:** 2026-08-28
- **Series:** R (Unique-export operator co-export, post-0106)
>
> **Closes:** `D-0071-also-eml`.
> **HITL:** none required. Synthetic fixture + existing `unique_eml_depth` / aspose pack proofs are enough. No INC* unique-pst+also-eml soak (`D-0094-inc-resmoke` stays operator HITL).
>
> **Last-PR fold-in (2026-08-28):** PRs **#103, #102, #101, #100**. Disposition in §2.8. No Cursor/Bugbot comments in that window. Origin residual is **D-0071-also-eml** (0071 clap accepted the flag and warned).
>
> **Review fold-in (2026-08-29):** `opencode-review.md` + `agy-review.md`. Disposition in §2.9 and `foldin-note.md`. Lock: helper takes real `ScanSummary` + `scan_ok` (never fabricate a clean scan); helper takes `soft_skip_attach_records` + promoted marks; combined exit uses 0078 integer precedence (`130 > 1 > 65 > 64 > 0`, **not** raw `u8` max); copy `method5_chain` (no cross-crate import); cancel during PST write skips also-eml; no `skip_serializing_if` on `also_eml_*`; also-eml guard vs PST volume siblings.
>
> This ID was reserved in Series Q notes for Hermes Series O. **It is not stolen for frontend.** Unique-eml nested MIME shipped in **0106**, which unblocks honest co-export. Frontend if started uses **0108+**.

---

## 1. Objective

Make `pst-dedup unique-pst --also-eml <dir>` write a real unique-EML pack from the **same keep-set winners** as the unique-PST, using the 0106 unique-eml writer (reconstructed nested RFC 5322, attach ledger, `--max-embedded-depth`). Stop warning-and-ignoring the flag.

Today the flag resolves the path, logs `warning: --also-eml is accepted but not implemented (D-0071-also-eml residual); ignoring`, and writes no EML. Operators who pass it get PST only. Before 0106 that was the safer lie (unique-eml dumped MAPI as `message/rfc822`). After 0106 the EML pack is counsel-defensible nested MIME, so the remaining operator gap is one command → both packs, **no second scan / no second keep-set**.

This advances unique-export **defensibility**: a documented flag must produce the pack it names, from the same winners as the PST, with independent EML artifacts and honest combined exit — never a silent no-op, never a re-dedupe that can diverge.

---

## 2. Context (read before starting)

### 2.1 Diagnosis (`D-0071-also-eml`, still live)

Deferred row: `--also-eml` co-export unique-eml pack alongside unique-pst. Flag accepted; co-export not wired in P0 (operators can run `unique-eml` separately).

Live confirmation 2026-08-28, `main` @ `65112ec` (re-verify line numbers at execute):

| Surface | State |
|---|---|
| Clap | `UniquePstArgs.also_eml: Option<PathBuf>` (`unique_pst_cmd.rs` ~148). Help: “Optional co-export unique-eml pack directory (soft residual; may be ignored).” |
| Runtime | `UniquePstCliArgs.also_eml` copied ~505. `run_unique_pst_with_options` ~1388: `resolve_cli_path_maybe_missing` then `tracing::warn` + `emit_log` “accepted but not implemented”; **no write**. |
| Path guard | `guard_unique_pst_paths` does **not** take the also-eml dir. Overlap with `--out` / report-dir is unchecked. |
| Summary | `UniqueExportSummary` (`Serialize` only, ~751) has **no** also-eml keys. `ExportSection.max_embedded_depth` already always-present (0101). |
| Test | `log_and_progress_callbacks_fire` (~4108) sets `also_eml: Some(also_eml_unused)` + `no_attachments: true` and **asserts** the unimplemented warning. **Must fail on HEAD** after this track. |
| unique-eml | `run_unique_eml` always `run_scan` + `resolve_groups` + promote finalize `|_msg\| Ok(())` then re-materialize + `materialize_nested_for_winner` + `write_canonical_eml` (0106). `prepare_out_dir` is **private** (~922). Volume defaults: `files_per_volume` 10000, prefix `VOL`. |
| unique-pst keep-set | After `finalize_with_materialize_opts`, `keep_set = resolved.to_keep_set()` (~2030). CanonicalMessage is converted to `PreparedWinner` and **consumed**. EML cannot reuse `PreparedWinner`. Re-materialize is required (same as standalone unique-eml write loop). |
| Family | unique-pst `effective_family`: `--no-attachments` forces `FamilyPolicy::ParentsOnly` (~1956). EML writer keys off `EmlWriteOpts.family_policy` (`parents_only` omits nested parts). |
| GUI | `unique_wizard.rs` ~367 `also_eml: None`. No checkbox. |
| Docs | `docs/unique-pst-export.md` ~67: “Soft residual (accepted; co-export may be ignored — see deferred)”. |
| Oracle | `SUMMARY_ALLOWLIST_KEYS` strips path-ish keys (`out`, `report_dir`, `summary_path`, …). 0102 lesson: do **not** put a product object key named `also_eml` on that list if counts live under it. |

### 2.2 Why now (after 0106)

0106 shipped unique-eml nested MIME: method-5 reconstructs RFC 5322 from `NestedCanonicalMessage`; honesty skip `ATTACH_EMBEDDED_UNPARSED` / `ATTACH_DEPTH_LIMIT`; `--max-embedded-depth` 1–8 default 3. Wiring `--also-eml` before that would have co-exported MAPI blobs labeled rfc822. Standalone `unique-eml` still works; this track is the one-command operator path, **same winners**.

### 2.3 RFC / MS-PST / crate APIs (plan-time)

**MS-PST:** N/A for new structures. Writer/reader nested export already shipped (0094/0101). This track is CLI orchestration.

**RFC 5322 / RFC 2046 §5.2.1:** already locked in 0106 (rfc822 wrapper CTE 8bit never base64; inner `Subject:` always). Also-eml **reuses** `write_canonical_eml` — do not fork MIME policy.

**Crate-registry API churn:** none expected. No new deps. Schema id `unique_export_report_v1` **not** bumped. `eml_pack_v1` / `keep_set_v1` **not** bumped.

### 2.4 Tools (plan-time)

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3863 pinned).
- `ai-brains sync query` / `recall` — 0106 unique-eml nested MIME shipped (PR #102); D-0067 narrowed not closed; frontend **0107+** at that pin. This pass uses **0107** for D-0071 co-export, so frontend moves to **0108+**.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` 0 pending / 0 unaudited drift (before this planning tx). `scan --impact` **LOW** (HEAD `65112ec`; dirty tree is skills + `agy-review.md` + `fixtures/keep_set_summary.json` + conductor sequencing/ROADMAP, not product crates). Hotspot `export_exit_0078.rs` is in scope for **tests** (new `also_eml:` literals stay `None` unless a test opts in).
- Ledger tx for this planning pass: `438773eb-22e6-414d-a132-5b34a7f1a583`.
- `C:\dev\Dedupe-plan.md` absent.

### 2.5 ai-brains decisions absorbed

| Memory | Use here |
|---|---|
| 0067 unique-eml MIME multipart; rfc822 8bit; UTC Date; no re-dedupe | Keep-set winners only; reuse 0106 writer |
| 0071 flag accepted / ignored | **This track wires it** |
| 0106 unique-eml nested MIME; D-0067 not closed; frontend 0107+ | Consume 0106 writer; do not close D-0067; frontend **0108+** |
| 0082 BCC opt-in (unique-pst) | Unchanged. also-eml uses **unique-eml** header policy (`Bcc:` from `display_bcc` when present), not unique-pst suppress |
| 0078 exit 64/65/130 | No new integers. Combined exit = worse of PST classify and also-eml classify |
| 0102 oracle `"inputs"` strip | Path key `also_eml_out` may join the allowlist; **do not** nest counts under a stripped key named `also_eml` |

### 2.6 How this advances the north star

Counsel-facing unique-export must be honest. A flag that names a second deliverable and then discards it is a documented no-op. After 0106 the EML pack is real nested MIME; co-export from the **same keep-set** is the defensible one-command path. Re-running `unique-eml` (second scan) can theoretically diverge; this track forbids that.

### 2.7 Why not frontend / HNBITMAPHDR / matter children / reader-buffer / BCC-default

- Hermes Series O (Tauri/Leptos) was reserved at 0107+ **if started**. North star is unique-export honesty, not UI polish. `ROADMAP.md` still locks Desk as native egui. Frontend IDs start at **0108**.
- `D-0067-embedded-depth` residual is matter/Relativity child-document extract — too large for this CLI glue track; **do not close**.
- `D-0100-hn-bitmap-hdr`: fail-closed until a corpus hits the error.
- `D-0079-reader-buffer`: pst-reader 64 KiB `BufReader` amplification — separate reader track.
- No BCC-default track (0082 unique-pst opt-in stays). also-eml does **not** apply PST BCC suppress to EML.

### 2.8 Last-PR Cursor comments (merged #103, #102, #101, #100)

Skill: last 2–4 merged product PRs.

| PR | Comment | Verdict |
|---|---|---|
| **#103** (0106 docs merge record) | No review / issue / inline comments. | n/a |
| **#102** (0106 unique-eml nested MIME) | No review / issue / inline comments. Codex P1/P2 (`attachments_incomplete`, `source_msg_nid.unwrap_or(0)`) fixed pre-merge (`fde5758`). | n/a — 0106 Completed |
| **#101** (0105 docs) | No comments. | n/a |
| **#100** (0105 window-edge) | No comments. | n/a |

Nothing else to mint. Origin work is **D-0071-also-eml** (this track). No BCC-default track. No HNBITMAPHDR track. Frontend stays **0108+**.

### 2.9 Dual-AI review disposition (2026-08-29)

Reviews: `opencode-review.md` (Ready after M1–M2 + m1–m5) and `agy-review.md` (PASS; restates the Ready plan). Neither asked to reopen unique-eml nested MIME, BCC default, HNBITMAPHDR, matter children, or frontend.

Live re-check this fold-in @ `65112ec`: unique-eml classify uses `evaluate_exit_policy` → `ExportOkInput.scan_ok` (`unique_eml_cmd.rs:503` / `:546`) and clones `outcome.summary` into `{out}/summary.json` (`:599`); Mode A ledger drains `resolved.soft_skip_attach_records` (`:322`) plus `keep_set.winners[].promoted_from_failure` (`:318`); `to_keep_set(&self)` leaves `resolved` alive; `classify_export` integer precedence is cancelled → 130, hard fail → **1**, risk → 65, partial → 64 (`export_outcome.rs:150-151`); `CliExit::Generic = 1`; neighboring `UniqueExportSummary` Options use `skip_serializing_if = "Option::is_none"` (`:800-807`); `guard_unique_pst_paths` checks `volume_path_for(out, 2..=MAX)` (`:3673-3682`); `method5_chain` lives in `unique_pst_depth.rs:39` (integration tests are separate crates). unique-eml `RiskGate` defaults Off + `PreflightRecommendation::Ok` (`:556-565`).

| Id | Source | Severity | Disposition | Spec landing |
|---|---|---|---|---|
| opencode-M1 | opencode-review.md | Major | **Agree — fold** | Helper takes unique-pst’s real `ScanSummary` + `scan_ok` (`evaluate_exit_policy` result). `{also_eml}/summary.json` `scan` is that summary — **never** a fabricated clean scan / hardcoded `scan_ok=true`. |
| opencode-M2 | opencode-review.md | Major | **Agree — fold** | Helper takes `&resolved.soft_skip_attach_records` (still alive after `to_keep_set(&self)`) and marks promoted winners from `keep_set.winners[].promoted_from_failure`. EML ledger Mode A rows for those loci; no silent drop. Not full CSV ⊇ of PST writer events (different surface). |
| opencode-m1 | opencode-review.md | Minor | **Agree — partial** | Combined exit uses **0078 integer precedence**, not raw `u8` max (`64 > 1` would hide `CliExit::Generic`). Rank: `130 > 1 > 65 > 64 > 0`. EML classify: unique-pst `fail_on_partial_fidelity` / `allow_partial_fidelity`; EML `RiskGate::Off` + `PreflightRecommendation::Ok` (pack has no `export_risk`; PST classify already carries the risk gate into combined worse-of). |
| opencode-m2 | opencode-review.md | Minor | **Agree — fold** | `method5_chain` is a **copy** (or new `tests/common/mod.rs`). Integration tests cannot import each other. |
| opencode-m3 | opencode-review.md | Minor | **Agree — fold** | `cancelled` at the also-eml gate (including cancel **during PST write**) → skip also-eml; `also_eml_ran=false`; exit stays 130 from PST classify. Pre-created empty also-eml dir may remain. |
| opencode-m4 | opencode-review.md | Minor | **Agree — fold** | The six `also_eml_*` fields: **no** `#[serde(skip_serializing_if)]`. Do not copy the adjacent `max_volume_bytes` / `decision_csv` pattern. Flag-absent `also_eml_out` JSON **null** must appear. |
| opencode-m5 | opencode-review.md | Minor | **Agree — fold** | Also-eml dir must not contain a planned PST volume sibling (`volume_path_for(out, 2..=MAX)` `is_same_or_under` also-eml). Fail closed before scan. |
| opencode-O1 | opencode-review.md | Opportunity | **Decline** | Pin-count 3863 vs 3864 is cosmetic. |
| opencode-O2 | opencode-review.md | Opportunity | **Already covered** | Volume defaults 10000 / `VOL` already locked in §3.4. |
| agy-0107-1 | agy-review.md | — | **Already covered** | Shared helper + re-materialize. |
| agy-0107-2 | agy-review.md | — | **Already covered** | Path guard + `prepare_out_dir` before scan. |
| agy-0107-3 | agy-review.md | — | **Already covered** | Cancel during **also-eml** → quarantine EML dir only, `also_eml_ran=true`, exit 130. Distinct from m3 (cancel during PST write skips also-eml). |
| agy-0107-4 | agy-review.md | — | **Agree — partial** | Restated `130 > 65 > 64 > 0`; upgraded by m1 to include Generic `1`. |
| agy-0107-5 | agy-review.md | — | **Already covered** | Oracle `also_eml_out` only. |
| agy-0107-6 | agy-review.md | — | **Already covered** | Callback warning invert. |

**Declined / not locked**

- Raw numeric `max(u8)` over `CliExit` (hides Generic=1 under Partial=64).
- Feeding unique-pst `export_risk` into `{also_eml}/summary.json` classify (EML pack describes EML; combined worse-of still surfaces PST 65).
- Full EML-ledger ⊇ PST-ledger CSV (writer CRC / stream events are PST-only). Mode A soft-skip + promoted marks are the honesty subset.
- Minting `tests/common/mod.rs` as **required** (copy of `method5_chain` is enough).
- Closing `D-0067-embedded-depth`.
- BCC-default track / unique-pst suppress on EML.
- Frontend **0108+**.

---

## 3. In scope

1. **Wire `--also-eml <dir>`:** When `UniquePstCliArgs.also_eml` is `Some`, after unique-pst keep-set is finalized **and** PST write has produced a keep-set (including attach-soft-fail / verify-failed jobs), write a unique-EML pack into that directory from **`keep_set.winners` only**. No second `run_scan`. No second `resolve_groups`. No call to `run_unique_eml` (that re-scans).
2. **Shared write helper:** Extract the unique-eml **write-from-keep-set** loop (re-materialize → nested extract → `VolumePackWriter` → `write_canonical_eml` → pack manifest + `{dir}/summary.json` + attach ledger) into a `pub`/`pub(crate)` function in `unique_eml_cmd.rs` (name implementer-choice, e.g. `write_eml_pack_from_keep_set`). Standalone `run_unique_eml` keeps scan+resolve+promote then calls the helper. unique-pst also-eml calls the same helper with the in-memory `KeepSet` + a `PstMaterializer` / `PstAttachStreamSource` (reuse unique-pst `PstHandleCache` when practical). **Required helper inputs** (beyond the Phase-1 list in the original plan): unique-pst `ScanSummary` + `scan_ok` (`evaluate_exit_policy` result — **never** fabricate a clean scan); `&resolved.soft_skip_attach_records` (or a clone; `to_keep_set(&self)` leaves `resolved` alive); promoted marks from `keep_set.winners[].promoted_from_failure`; unique-pst `fail_on_partial_fidelity` / `allow_partial_fidelity`; cancel flag. `{also_eml}/summary.json` is `UniqueEmlSummaryOut` shape: `scan` = that `ScanSummary`.
3. **Family / depth / ledger inherited from unique-pst:**
   - `EmlWriteOpts.family_policy` = unique-pst `effective_family` (`--no-attachments` → `ParentsOnly`, else `args.family_policy`).
   - `max_embedded_depth` = unique-pst effective clamp (same 0101/0106 1–8, default 3). Call `materialize_nested_for_winner` with **that same** value.
   - Attach ledger at `{also_eml}/export_attachments.csv` using unique-pst `--attach-ledger` / `--attach-ledger-max-rows` / `--ledger-path-mode`. **Separate file** from `{report-dir}/export_attachments.csv`. Do not merge rows into the PST ledger. **Before** the write loop, drain Mode A honesty the same way standalone unique-eml does (`unique_eml_cmd.rs` ~314–355): `mark_promoted_winner` for `promoted_from_failure` winners, then `enqueue_soft_skip_row` for each `soft_skip_attach_records` entry. Omitting that is a silent EML-ledger drop.
4. **Volume batching:** unique-pst has no `--files-per-volume`. Use unique-eml defaults (`files_per_volume` 10000 after clamp, prefix `VOL`). Do **not** add unique-pst clap for volume this track.
5. **Path guards (fail closed, before scan):** extend `guard_unique_pst_paths` (or a sibling) so `--also-eml`:
   - is a directory path (create later); refuse if it exists as a **file**;
   - is not equal to an input PST, `--out`, `--report-dir`, `decision_csv`, `keep_set_json`, or `integrity_csv`;
   - is not nested under an input PST; does not contain an input PST;
   - is not the same as or under `report_dir`; `report_dir` is not under also-eml;
   - is not equal to `--out` (PST file) and not a parent that would delete the PST on clear;
   - does not contain a planned unique-pst **volume sibling** (`volume_path_for(out, 2..=MAX_VOLUME_SIBLING_INDEX)` is not `is_same_or_under` the also-eml dir). `--overwrite` clear of `--also-eml C:\x` with `--out C:\x\u.pst` would delete `u_vol002.pst` before the PST volume guard runs.
   Resolve via `resolve_cli_path_maybe_missing` **once**; use the resolved path everywhere.
6. **Overwrite:** `--overwrite` applies to the also-eml dir the same way unique-eml `prepare_out_dir` applies to `--out` (refuse non-empty without flag; clear contents with flag). Share `prepare_out_dir` as `pub(crate)` rather than forking the clear logic. unique-pst `--overwrite` already covers `--out` / report-dir; one flag covers also-eml too (no second overwrite flag).
7. **When also-eml runs / skips:**
   - Flag absent → skip. `also_eml_ran = false`. **No** unimplemented warning.
   - Scan / resolve / materialize never produced a keep-set (hard fail or cancel before `to_keep_set`) → skip also-eml; do not create a fake pack.
   - `cancelled` is already set at the also-eml gate (cancel **during PST write**, or any cancel before also-eml starts) → **skip** also-eml; `also_eml_ran=false`; no unimplemented warning; do not write pack contents (pre-created empty dir from up-front `prepare_out_dir` may remain); process exit stays **130** from PST classify.
   - Keep-set exists, PST write ran, **not** cancelled (including `ATTACH_SOFT_FAIL` / `VERIFY_FAILED`) → **run** also-eml.
   - Cancel during also-eml → quarantine **only** the also-eml dir (0078); **do not** quarantine the unique-PST / report-dir; `also_eml_ran=true`; process exit **130**.
8. **Failure isolation:** also-eml hard-fail does **not** delete or quarantine unique-PST volumes or `{report-dir}`. Partial EML follows unique-eml 0078 (`artifact_state` on `{also_eml}/summary.json`).
9. **Combined process exit:** no new integers. After both writes, process `exit_code` / `exit_reason` = **0078 integer precedence** over unique-pst classify and also-eml classify: **`130 > 1 (Generic hard fail) > 65 > 64 > 0`**. Do **not** use raw `max(u8)` (`PartialFidelity=64` would hide `Generic=1`). Merge `exit_reason` (stable, worst-first, no duplicate codes). unique-pst `summary.json` `ok` follows the **combined** exit (`ok` true only when combined is 0). `{also_eml}/summary.json` remains unique-eml’s own classify (its `ok` / `exit_code` describe the EML pack only). **EML classify inputs:** `scan_ok` + `scan` payload from unique-pst’s real scan (`evaluate_exit_policy`); `fail_on_partial` from unique-pst `--fail-on-partial-fidelity` / `--allow-partial-fidelity`; `RiskGate::Off` + `PreflightRecommendation::Ok` (same as standalone unique-eml — the pack has no `export_risk`; PST classify already carries the risk gate into combined worse-of). Count mismatch (`eml_written != unique`) is hard-fail Generic=1 on the EML side when `export_partial` / count mismatch is set, same as `unique_eml_cmd.rs:506-549`.
10. **Summary honesty (`unique_export_report_v1` id not bumped).** `UniqueExportSummary` is Serialize-only (no `serde(default)` trap). Always serialize (**no** `skip_serializing_if` on any of the six keys — do **not** copy `#[serde(skip_serializing_if = "Option::is_none")]` from neighboring `max_volume_bytes` / `decision_csv` / `keep_set_json` / `error`):
    - `also_eml_out: Option<String>` — resolved path when flag set, `null` when not;
    - `also_eml_ran: bool`;
    - `also_eml_eml_written: u64`;
    - `also_eml_attach_parts_failed: u64`;
    - `also_eml_embedded_messages_written: u64`;
    - `also_eml_exit_code: u8` — `0` when not run.
    Oracle: add **`"also_eml_out"`** to `SUMMARY_ALLOWLIST_KEYS` (path, like `out`). Do **not** add `also_eml_ran` / count keys / `also_eml_exit_code` (those must attest). Do **not** add a generic key `"also_eml"` that would strip a nested product object (0102).
11. **Progress / logs:** `stage=also_eml` progress ticks. Human stderr prints also-eml path + `eml_written` / attach fail counts near unique-pst attach counts. **Delete** the “accepted but not implemented” warn path.
12. **Clap help:** replace “soft residual; may be ignored” with: co-export unique-eml pack from the same keep-set; requires a directory; `--overwrite` replaces a non-empty dir; nested MIME follows `--max-embedded-depth`.
13. **BCC split (document, do not “fix”):** unique-pst still default-suppresses BCC on the PST (0082). also-eml EML still writes `Bcc:` from `display_bcc` when present (0106 unique-eml policy). `--include-bcc-recipients` does **not** change EML headers. Same as running `unique-eml` separately.
14. **Tests** (synthetic only; see §7 / §10):
    - `log_and_progress_callbacks_fire` **must fail on HEAD** after invert: no unimplemented warning; `also_eml` dir contains `.eml` files (test already uses `no_attachments: true` — parents-only pack is enough).
    - New `crates/pst-dedup-cli/tests/unique_pst_also_eml.rs` (locked name): aspose `--also-eml` winner count matches EML files; `summary.json` `also_eml_ran == true`; `{also_eml}/manifest.json` `eml_pack_v1`; no unimplemented warning.
    - Path overlap (`--also-eml` equal `--out` or report-dir) → usage error before write.
    - Non-empty also-eml dir without `--overwrite` → usage error.
    - Method-5 chain: **copy** `method5_chain` from `unique_pst_depth.rs` (or add `tests/common/mod.rs`). Integration tests are separate crates and **cannot** import each other. unique-pst `--also-eml` produces inner `Subject:` rfc822. Do **not** inject `--no-attachments` on that test.
    - Mode A ledger: helper/CLI test with `promoted_from_failure` + one `SoftSkipAttachRecord` → `{also_eml}/export_attachments.csv` contains that `(msg_nid, reason_code)` (and `mark_promoted_winner` applied). Do **not** require full EML CSV ⊇ PST writer events.
    - Flag absent: `also_eml_ran == false`, `also_eml_out` JSON **null** (key present), no extra dir created.
    - Existing unique-eml tests + `unique_eml_depth` stay green. Existing unique-pst tests that pass `also_eml: None` stay green.
    - export_oracle tests stay green (`also_eml_out` stripped).
15. **Docs:** `docs/unique-pst-export.md` flag row; `docs/unique-eml-import.md` one sentence that `unique-pst --also-eml` writes the same pack from the unique-pst keep-set; CHANGELOG Unreleased; close `D-0071-also-eml`.

---

## 4. Out of scope (do NOT do here)

- Calling `run_unique_eml` (second scan / second keep-set).
- `--also-pst` on unique-eml.
- unique-eml `--files-per-volume` clap on unique-pst.
- GUI wizard `--also-eml` checkbox (`D-0072` / `D-0073-gui` class).
- Matter / Relativity child-document extract (`D-0067-embedded-depth` — **do not close**).
- Re-parsing by-value `message/rfc822` into `NestedCanonicalMessage`.
- Identity hash `MAX_EMBEDDED_IDENTITY_DEPTH` (locked 3).
- unique-pst nested write / `--max-embedded-depth` semantics (already 0094/0101).
- unique-pst BCC default / `--include-bcc-recipients` (0082 stays).
- HNBITMAPHDR (`D-0100-hn-bitmap-hdr`).
- Per-event attach CRC (`D-0099-attach-crc-job-level`).
- `D-0079-reader-buffer` / `D-0079-stream-prepare` / `--jobs`.
- `D-0077-poly-fingerprint`.
- Frontend / Hermes Series O (**0108+**).
- COM Outlook; client PSTs in git; in-tool ScanPST / CRC repair.
- Cloud attach hydration (`D-0067-cloud-attaches`).
- Sharing `PreparedWinner` / `WriteMessage` as EML input (wrong type).

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0106 Completed (`write_canonical_eml` nested MIME + unique-eml `--max-embedded-depth` + `materialize_nested_for_winner` on the unique-eml write loop). 0071 Completed (flag exists). 0089 Completed (`EmlAttachEvent` → CSV). Verified @ `65112ec`.
- **P2 (soft):** 0078 classify/quarantine vocabulary — reuse, no new exits.
- *Verified to date:* also-eml warns and ignores; `prepare_out_dir` is private in `unique_eml_cmd.rs`; `UniqueExportSummary` has no also-eml keys; callback test asserts the warning.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Implementer calls `run_unique_eml` and re-dedupes | DoD-1: same `KeepSet.winners` (path+nid) as PST; no second scan. Test asserts EML count == `keep_set.stats.unique`. |
| Second keep-set diverges from PST | Forbidden. Helper takes the in-memory keep-set. |
| Reuse `PreparedWinner` as EML | Wrong type. Re-materialize CanonicalMessage like standalone unique-eml. |
| `open_attach_body` / nested skip regressions | Reuse 0106 writer; method-5 tests on co-export path. |
| also-eml failure deletes PST | Isolation lock: quarantine also-eml dir only. |
| Combined exit swallows EML 64 | 0078 precedence; test PST-clean + EML attach-fail → process 64, PST files remain. (Synthetic method-5 no-DTO skip is enough.) |
| Combined exit uses raw `max(u8)` | `64 > 1` hides Generic hard-fail. Use `130 > 1 > 65 > 64 > 0`. PST-clean + EML hard-fail → combined **1**, `ok=false`, PST kept. |
| `{also_eml}/summary.json` fabricates `scan_ok=true` | Helper takes unique-pst `ScanSummary` + `evaluate_exit_policy` result. |
| EML ledger drops Mode A rows | Helper takes `soft_skip_attach_records` + `promoted_from_failure`; drain before write loop. |
| `also_eml_out` omitted when None | No `skip_serializing_if` on the six keys. |
| `--also-eml` parent of `--out` wipes volume siblings | Guard `volume_path_for(out, 2..=MAX)` vs also-eml dir. |
| Cancel during PST write still runs also-eml | Skip; `also_eml_ran=false`; exit 130. |
| Oracle strips counts | Allowlist **only** `also_eml_out`. 0102: no generic `"also_eml"` object key. |
| `prepare_out_dir` fork drifts | `pub(crate)` share. |
| Path overlap wipes report-dir | Guard before scan. Usage-error tests. |
| Callback test still expects warning | Invert; **must fail on HEAD**. |
| GUI wizard grows a checkbox | Out of scope; keep `also_eml: None`. |
| unique-pst BCC suppress applied to EML | Document split; EML still writes `Bcc:` when `display_bcc` present. |
| `--jobs` / stream-prepare | Out of scope. Sequential also-eml after PST write. |
| Frontend ID collision | This track **is** 0107. Frontend **0108+**. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Co-export from the same keep-set:** `unique-pst --also-eml <dir>` writes a unique-EML pack (VOL batch, `eml_pack_v1` manifest, `{dir}/summary.json`, attach ledger per unique-pst ledger mode) from **`keep_set.winners`**. No second scan/resolve. No `run_unique_eml`. Nested MIME is 0106 (`materialize_nested_for_winner` + `write_canonical_eml` with the same effective `--max-embedded-depth` and `effective_family`). Helper takes real `ScanSummary` + `scan_ok`, `soft_skip_attach_records`, and `promoted_from_failure` marks. Path guards + `--overwrite` as §3 (including volume siblings). Unimplemented warning **gone**. also-eml failure does not quarantine PST. Combined process exit is 0078 precedence (`130 > 1 > 65 > 64 > 0`). Cancel during PST write → skip also-eml (`also_eml_ran=false`). Cancel during also-eml → also-eml quarantined, PST kept, exit 130, `also_eml_ran=true`. No `unwrap`/`expect` in production. Source PSTs read-only.
- [ ] **DoD-2 — Summary + tests:** `UniqueExportSummary` always has `also_eml_out` / `also_eml_ran` / `also_eml_eml_written` / `also_eml_attach_parts_failed` / `also_eml_embedded_messages_written` / `also_eml_exit_code` (no `skip_serializing_if`; flag-absent `also_eml_out` JSON **null**). Schema id not bumped. Oracle strips only `also_eml_out`. `{also_eml}/summary.json` `scan` matches unique-pst scan (not fabricated). Tests in §10.2: callback warning invert **must fail on HEAD**; `unique_pst_also_eml.rs` aspose + overlap + overwrite + copied method-5 chain; Mode A ledger rows; flag-absent nulls; existing unique-eml / unique-pst / oracle tests stay green. No client PSTs in git.
- [ ] **DoD-3 — Docs:** `docs/unique-pst-export.md` flag row; `docs/unique-eml-import.md` co-export sentence; CHANGELOG Unreleased; `D-0071-also-eml` **closed**. `D-0067-embedded-depth` stays open.
- [ ] **DoD-4 — Recorded:** `review.md`; registry **Completed**; ledger commit (`FEATURE` on `crates/pst-dedup-cli` at implement). No HITL required.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
$env:CARGO_TARGET_DIR = 'C:\dev\Dedupe\target'
cargo test -p pst-dedup-cli --lib unique_pst_cmd
cargo test -p pst-dedup-cli --test unique_pst_also_eml
cargo test -p pst-dedup-cli --test unique_eml
cargo test -p pst-dedup-cli --test unique_eml_depth
cargo test -p pst-dedup-cli --test unique_pst_depth
cargo fmt --all --check
cargo clippy -p pst-dedup-cli --all-targets -- -D warnings
# before implement-track publish:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Filter names re-verify at execute (`unique_pst_also_eml` is the locked new integration module unless execute finds a collision). No operator INC* command. Do **not** use a helper that injects `--no-attachments` on the nested-MIME co-export test (0101/0106 lesson). The callback unit test may keep `no_attachments: true`.

---

## 9. Deferred roll (mandatory)

Entire `docs/deferred.md` scanned 2026-08-28. Related open rows:

| Row | Disposition |
|---|---|
| **D-0071-also-eml** | **Absorb / close.** Wire `--also-eml` from the unique-pst keep-set. |
| **D-0067-embedded-depth** | **Decline.** unique-eml MIME shipped / 0106; residual matter children + 32 MiB + cap 8. **Do not close.** |
| **D-0067-long-path** | **Decline.** Path budget residual. |
| **D-0067-cloud-attaches** | **Decline.** No invent file bytes. |
| **D-0067-gui-keepset** | **Decline.** GUI unique-pst is primary (0072). No wizard also-eml checkbox this track. |
| **D-0094-inc-resmoke** | **Decline.** Operator unique-pst HITL. Not this CLI. |
| **D-0100-hn-bitmap-hdr** | **Decline.** Fail-closed until a corpus hits it. |
| **D-0099-attach-crc-job-level** | **Decline.** 0099 declined per-event split. |
| **D-0077-poly-fingerprint** | **Decline.** Later reader track. |
| **D-0079-reader-buffer** | **Decline.** pst-reader buffer polish. |
| **D-0079-stream-prepare** | **Decline.** Phase C / `--jobs` plumbing. |
| **D-0088-usgovcloud-microsoft-tld** | **Decline.** |
| **D-0073-gui** / **D-0074-gui** / **D-0078-gui** | **Decline.** Wizard checkbox not this track. |
| **D-0062-codesign** | **Decline.** Release ops. |
| Other `docs/deferred.md` rows | **Decline** — not unique-pst also-eml co-export. |

Med/high never parked here. No BCC-default track. Frontend **0108+**. Fold-in (2026-08-29) did not change these dispositions.

---

## 10. Product locks (do not reopen)

1. Never mutate source PST / Purview files.
2. Never commit client PSTs, `output/`, `evidence/`, or matter folders with client mail.
3. No `unwrap` / `expect` in production.
4. Crate boundary: MIME writer stays in `dedup-engine::eml_pack`; CLI orchestration in `pst-dedup-cli`. Do not teach `pst-writer` EML policy. Do not change `pst-reader` APIs.
5. Unique-export: no silent attach/count drops. Same keep-set as the PST. also-eml skip/fail is ledgered on the **EML** pack, not silently dropped.
6. No in-tool ScanPST / CRC repair of evidence.
7. unique-pst `--include-bcc-recipients` default **off** (untouched). also-eml EML continues to emit `Bcc:` from `display_bcc` when present.
8. Identity hash depth stays **3**.
9. Per-nest byte budget stays **32 MiB**. Product ceiling stays **8**.
10. Do not implement HNBITMAPHDR.
11. Do not start Hermes Series O in this folder.
12. Do not bump `unique_export_report_v1` / `eml_pack_v1` / `keep_set_v1` schema ids.
13. No new process exit integers. Combined uses the existing 0078 set: **130 / 65 / 64 / 1 (Generic) / 0**. Rank is 0078 integer precedence, not raw `u8` max.
14. Do not call `run_unique_eml` from unique-pst.

### 10.1 Locked fix (closed)

**Option: after unique-pst keep-set + PST write, write EML from that keep-set.**

1. Path-guard + `prepare_out_dir` the also-eml dir up front (same `--overwrite`).
2. unique-pst proceeds as today through keep-set + PST volumes + PST attach ledger.
3. If keep-set exists, also-eml was requested, and **not** `cancelled` (cancel during PST write skips): `stage=also_eml`; `write_eml_pack_from_keep_set(...)` with `effective_family`, same `max_embedded_depth`, reused handle cache, unique-pst ledger mode, **real `ScanSummary` + `scan_ok`**, **`soft_skip_attach_records`**, promoted marks, unique-pst `fail_on_partial` flags. EML risk = Off/Ok.
4. Re-materialize each winner (CanonicalMessage). Nested extract on that loop. Do not reuse `PreparedWinner`. Drain Mode A ledger rows **before** the write loop.
5. Combined classify after both writes using **0078 integer precedence** (`130 > 1 > 65 > 64 > 0`). PST artifacts never quarantined because EML failed.
6. Summary always-present also-eml keys (**no** `skip_serializing_if`). Oracle strips `also_eml_out` only.

**Declined:** `run_unique_eml` second pass.

**Declined:** sharing `WriteMessage` / `PreparedWinner` as EML input.

**Declined:** applying unique-pst BCC suppress to EML.

**Declined:** new exit integers. **Declined:** raw `max(u8)` over `CliExit`.

**Declined:** fabricating a clean `scan` / `scan_ok=true` on the EML pack.

**Declined:** nesting counts under a key named `also_eml` that is also on `SUMMARY_ALLOWLIST_KEYS`.

**Declined:** GUI checkbox.

**Declined:** closing `D-0067-embedded-depth`.

**Declined:** frontend as this ID.

### 10.2 Test fixtures (locked)

Unit / lib (`unique_pst_cmd.rs`):

- **Warning invert (fail on HEAD):** `log_and_progress_callbacks_fire` currently asserts `warning` + `also-eml`. After: **no** “not implemented” line; `also_eml_unused` is a directory with ≥1 `.eml` (parents-only because `no_attachments: true`); progress includes `also_eml` or `done`.

CLI (`crates/pst-dedup-cli/tests/unique_pst_also_eml.rs`, locked name):

- Aspose `unique-pst --also-eml <dir> --qc-level off` (or equivalent fixture helper): `{dir}/VOL001` (or `VOL*` ) `.eml` count == `summary.keep_set.stats.unique`; `{dir}/manifest.json` schema `eml_pack_v1`; unique-pst `summary.json` `also_eml_ran == true`, `also_eml_eml_written` equals that count, `also_eml_out` non-null; logs have no “not implemented”.
- `--also-eml` equal to `--out` or `--report-dir` → usage error; no PST write.
- Non-empty also-eml dir without `--overwrite` → usage error.
- Flag omitted: `also_eml_ran == false`, `also_eml_out` JSON null; no stray pack dir.
- Method-5 chain (**copy** `method5_chain` from `unique_pst_depth.rs:39`, or `tests/common/mod.rs` — do **not** `use` another integration crate): `--also-eml` pack contains `Content-Type: message/rfc822` and inner subject; do **not** pass `--no-attachments`.
- Mode A: `promoted_from_failure` winner + `SoftSkipAttachRecord` → EML `export_attachments.csv` has that `msg_nid` + `reason_code`.
- Existing `unique_eml.rs` / `unique_eml_depth.rs` / `unique_pst_depth.rs` stay green.

### 10.3 Names (do not conflate)

| Name | Owner | Role |
|---|---|---|
| `unique-pst --also-eml` | unique-pst CLI | **This track** — co-export dir from the unique-pst keep-set |
| `unique-eml --out` | unique-eml CLI | Standalone pack (still scans). Unchanged. |
| `EmlWriteOpts::max_embedded_depth` | eml_pack + both CLIs | 0106 / 0101 clamp [1, 8], default 3 |
| `WritePstOpts::max_embedded_depth` | writer + unique-pst | Already shipped. Do not change. |
| `MAX_EMBEDDED_IDENTITY_DEPTH` | `pst-reader` | 0090 hash recursion, **locked 3** |
