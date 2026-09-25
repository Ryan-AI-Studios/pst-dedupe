# 0139 — Source-rank discoverability (`first_seen` path sort)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> **Do not change `first_seen` semantics** (sorted input-path order). No BCC-default.
> `--source-rank` stays opt-in. Do not steal **0100–0104**. Not frontend.

- **Track ID:** 0139-SourceRankDiscoverability
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** unique-pst keep-set policy. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-25).
- **Status:** Completed
- **Depends on:** **0066** keep-set · **0071** / **0075** `--source-rank` ladder
- **Spec authored:** 2026-09-24 placeholder
- **Fold-in:** 2026-09-25 `agy-review.md` + `opencode-review.md` (+ bundle `AI-review.md`)
- **Series:** X (INC0102784 CLI operator friction, readonly eval 2026-09-24)
- **Ledger category:** `FEATURE`

> **Closes / absorbs:** `D-0139-source-rank-discoverability`.
> **HITL (owner, not CI):** optional two-file INC* `keep-set --json` confirming one stderr note and `input_path_sort_order`. Never commit INC* PSTs.

---

## 1. Objective

Operators who pass a split Purview search (`INC0102784.pst` + `INC0102784-2.pst`) must see that default `first_seen` crowns by **sorted resolved absolute path**, so the `-2` file can win shared-MID groups unless `--source-rank` is set. 2026-09-24 keep-set: **8** groups decided by `source_rank` only when the flag was passed.

Deliver three discoverability surfaces on `keep-set` / `unique-pst` / `unique-eml` only: clap help, a top-level JSON `input_path_sort_order`, and one run-level stderr note. Ranking math stays as shipped.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

ASCII `'-'` (`0x2D`) sorts before `'.'` (`0x2E`). After `sort_input_paths`, `INC0102784-2.pst` precedes `INC0102784.pst`. Default `KeepPolicy::FirstSeen` uses `scan_order` assigned over that sorted list. `--source-rank` is the operator override; it is easy to miss.

Docs already tell the truth (`docs/unique-pst-export.md` honesty paragraph; runbook `first_seen` row). The CLI help, JSON envelope, and stderr do not.

### 2.2 Live facts (plan-time HEAD `791d230`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| Schema | Matter **41**. N/A this track. No `keep_set_v1` / `unique_export_report_v1` bump. |
| `KeepPolicy::FirstSeen` | `keepset.rs`: sorted input-path order, then scan index — not send time. |
| `path_compare_key` / `sort_input_paths` | Windows: lexicographic on **lowercased** absolute path; original path preserved for open. |
| Winner ladder | `rank_key`: fidelity → bcc → source → folder (or swapped) → policy → `path_key` → nid. New rungs are 0 when flags are off. |
| `source_rank_of` | Empty pattern list → `0` for every item (inert). Unmatched → `patterns.len()` (worst). |
| `prefer_path_policy_key` | Used **only** when `--policy prefer_path`. Does not change a `first_seen` tie. |
| `--policy` help | Already: `first_seen = sorted input-path order, not chronological send time` on keep-set / unique-pst / unique-eml. **Do not duplicate** that sentence. |
| `--source-rank` help | Short “ordered source preference…” only. This is the clap surface to extend. |
| `KeepSetSummaryOut` | No top-level path-order field. Sorted paths already live at `keep_set.created_from.input_files` (after `sort_input_paths`). |
| `UniqueExportSummary` | Top-level `inputs: Vec<String>` filled from sorted `paths`. Keep it. |
| `UniqueEmlSummaryOut` | No top-level `inputs`. Sorted paths in `keep_set.created_from.input_files`. Built in **two** sites in `unique_eml_cmd.rs`. |
| Export oracle | `SUMMARY_ALLOWLIST_KEYS` is name-based recursive strip. Root `/inputs` is **blanked** to `[]` (not allowlisted). D-0099/0102: never put `"inputs"` back on the allowlist. |
| `--also-eml` | Parent `unique-pst` calls `write_eml_pack_from_keep_set` with `emit_depth_limit_hint: false`. It does **not** call `run_unique_eml`. 0127 dual-hint precedent. |
| RI hint | `recoverable_items_hint` has no `note:` prefix; callers add it. unique-pst `emit_log` even under `--json`; keep-set / unique-eml `eprintln` only when `!json`. |
| Tests | `source_rank_ordered_primary_beats_dash2`; `source_rank_flips_winner_file_a_vs_a2`; help lists `--source-rank` as a flag name. |
| clap | Workspace `version = "4"`, `features = ["derive"]`. Lock **4.6.4**. docs.rs latest **4.6.7**. Derive `///` still becomes `--help`. |
| serde_json | Workspace `"1"`. Additive struct fields are backward-compatible for readers that ignore unknowns. |
| MS-PST | **N/A this track.** |
| Hotspot | `unique_pst_cmd.rs` is the densest CLI file. Keep unique-pst edits to help + one early emit + one summary field. |

### 2.3 Tools (plan-time 2026-09-25)

| Tool | Result |
|---|---|
| `ai-brains preflight --summary` | Vault live (project `93e74c21`). 5013 pins. |
| `ai-brains recall` / `sync query` | 0075: all ranking flags default off. Series X minted 2026-09-24 as Proposed placeholders; this fold-in makes **0139** Ready. |
| `ledgerful doctor --json` | `readyForPublish: true`, 0 block. Unrelated warn: phantom verify rows, completion model cold. |
| `ledgerful ledger status --compact` | 0 pending, 0 unaudited drift. |
| `ledgerful scan --impact` | `riskLevel: low`. Dirty tree is coordinator/conductor/docs. `unique_pst_cmd.rs` remains hotspot #1 — minimize edits there. |

### 2.4 Last-PR Cursor comments

Merged PRs **#153, #152, #151, #150**: inline comments empty; reviews empty; issue comments are Cursor Bugbot “usage limit reached” only. **Decline** — no 0139-owned finding.

### 2.5 Product locks

- Do not change `first_seen`, `sort_input_paths`, or the winner ladder.
- Do not default-on `--source-rank`, `--prefer-folder-class`, `--prefer-bcc-copy`, or `--prefer-path-contains`.
- `--prefer-path-contains` does **not** suppress the multi-input note (it is not a `first_seen` file-order override).
- `--rank-folder-class-first`, `--prefer-bcc-copy`, and non-`first_seen` policies do **not** suppress the note. Path order remains the last tie-breaker; `earliest_date` often falls through (export.md).
- Suppress the note only when at least one **non-empty** `--source-rank` pattern is present.
- `scan` and `dups` stay untouched: no note, no `input_path_sort_order`.
- No BCC-default. No source-PST mutation. No client PST in git. Schema 41.
- `KEEP_SET_SCHEMA` (`keep_set_v1`) and `UNIQUE_EXPORT_REPORT_SCHEMA` (`unique_export_report_v1`) stay. The new JSON key is a CLI wrapper field.
- Oracle: **remove** root `input_path_sort_order` in `normalize_summary_for_oracle`. Do **not** add the name to `SUMMARY_ALLOWLIST_KEYS`. Do **not** insert `[]` (parent packs omit the key; an empty array would still mismatch).
- unique-pst `inputs` remains and must equal `input_path_sort_order` (same sorted resolved paths).
- 0141 may strip `keep_set.winners` from stdout; it must **keep** wrapper `input_path_sort_order`.
- 0145 `--from-scan-json` (when planned) reuses `created_from.input_files` / this wrapper as the order used — do not invent a fourth source of truth here.

---

## 3. In scope

1. **DoD-1 — clap help.** Extend `--source-rank` (and, if needed, `KeepPolicy::FirstSeen` rustdoc) so `keep-set` / `unique-pst` / `unique-eml` `--help` names lexicographic path sort and the `-2.pst` vs `.pst` ASCII case. Leave the existing `--policy` first_seen sentence in place.
2. **DoD-2 — JSON `input_path_sort_order`.** Additive `Vec<String>` of **resolved absolute** paths after `sort_input_paths` on:
   - `KeepSetSummaryOut` (stdout and `keep_set_summary.json`)
   - `UniqueExportSummary` (stdout and `summary.json`); keep `inputs`
   - `UniqueEmlSummaryOut` (both construction sites; stdout and pack `summary.json`)
3. **DoD-3 — one stderr note.** Shared helper in `dedup_engine::keepset` next to `recoverable_items_hint`. Emit **once**, immediately after `sort_input_paths`, from `run_keep_set`, `run_unique_eml`, and the unique-pst command path. Never from `write_eml_pack_from_keep_set_inner`.
4. **Oracle.** Root-remove `input_path_sort_order` in `export_oracle.rs`. Unit test: parent JSON without the key vs HEAD JSON with absolute paths compare equal after normalize.
5. **Tests** in §7. CHANGELOG user-facing CLI line. Optional owner HITL.
6. **Docs (thin):** one sentence in `docs/unique-pst-export.md` that the CLI emits the run-level note. Do not rewrite the honesty paragraph.

### 3.1 Helper contract

```text
pub fn multi_input_source_rank_hint(
    input_count: usize,
    source_rank_patterns: &[String],
) -> Option<String>
```

- `None` when `input_count < 2`.
- `None` when any pattern is non-empty.
- `Some` otherwise. **No** `note:` prefix (callers add it, same as RI).
- Pinned inner text (tests may assert substrings `--source-rank`, `sorted path order`, `'-'`):

```text
{n} input PST(s) without --source-rank; first_seen and remaining ties use sorted path order (ASCII '-' sorts before '.', so -2.pst can crown before .pst)
```

Call sites:

| Command | Channel | `--json` |
|---|---|---|
| `keep-set` | `eprintln!("note: {hint}")` | **Still emit** (stdout stays JSON-only) |
| `unique-eml` standalone | same `eprintln!` | **Still emit** |
| `unique-pst` | `emit_log` with `note: {hint}` (existing `unique-pst: ` prefix) | **Still emit** (stderr / `on_log`) |
| `unique-pst --also-eml` | parent unique-pst only | Pack inner silent |
| `scan` / `dups` | none | n/A |

The note does not change `exit_code`, `fidelity`, or stdout bytes except the additive JSON field in DoD-2.

### 3.2 JSON naming contract

Existing sorted-path fields stay:

| Envelope today | Existing field | 0139 addition |
|---|---|---|
| keep-set `--json` | `keep_set.created_from.input_files` | top-level `input_path_sort_order` |
| unique-pst `--json` / `summary.json` | `inputs` | sibling `input_path_sort_order` (same values) |
| unique-eml `--json` / pack summary | `keep_set.created_from.input_files` | top-level `input_path_sort_order` |

Values are resolved-absolute, **not** user-typed argv order. Populate even for a single input (order used is still one path). The stderr note is the multi-input-only surface.

---

## 4. Out of scope

- Changing `first_seen` to “primary file first” or filename-without-directory sort.
- Desk GUI `--source-rank` lists (`D-0075-gui`).
- Unique-pst write speed (**0146**).
- `keep-set --json` winner-list size (**0141**).
- `scan` / `dups` JSON or stderr (**0140**, **0142**, **0145**).
- Nested `Desktop\Desktop` operator path.
- Default-on BCC / folder-class / source-rank.
- CRC / poly copy (**0143**), integrity CSV (**0144**).
- Frontend / chrome.

---

## 5. Preconditions

- **P1:** `sort_input_paths` still runs before `run_scan` on keep-set / unique-pst / unique-eml.
- **P2:** `--source-rank` still the only default-off rung that reorders cross-file `first_seen` crowns.
- *Verified 2026-09-25:* both true at `791d230`. Re-verify in Phase 0.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| New JSON key breaks parent↔HEAD oracle | Root-**remove** the key; unit test; never allowlist the name |
| Fourth path field confuses 0141 | Wrapper-only; 0141 keeps it when dropping winners |
| `--prefer-path-contains` false-suppresses the note | Predicate ignores that flag; CLI test with only prefer-path still notes |
| Dual stderr on `--also-eml` | Emit only in the three command runners, never pack inner; `matches(…).count() == 1` |
| `--json` stdout polluted | Note on stderr only; parse stdout as JSON in tests |
| unique-pst hotspot regressions | Minimal unique-pst diff; reuse keep-set CLI test as primary |
| Ranking accidentally changes | Existing `source_rank_*` tests must stay green; no ladder edits |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Help.** `keep-set` / `unique-pst` / `unique-eml` `--help` contains `--source-rank` **and** path-sort / `-2.pst` (or ASCII `'-'` before `'.'`) language. Existing `--policy` first_seen sentence remains; do not paste it a second time on `--source-rank`.
- [ ] **DoD-2 — JSON.** `--json` envelopes for those three commands include top-level `input_path_sort_order` equal to post-sort resolved paths. unique-pst `inputs` unchanged and equal to that array. Field present on on-disk keep-set summary and unique-pst `summary.json`. No keep-set / unique-export schema bump.
- [ ] **DoD-3 — Note.** ≥2 inputs and no non-empty `--source-rank` → exactly one stderr line containing the helper text (plus command prefix on unique-pst). `--source-rank` with a real pattern → zero matches. `--prefer-path-contains` alone → still one match. `--json` stdout is parseable JSON. unique-pst `--also-eml` → count `== 1`. Single-input runs emit no note. Exit codes unchanged vs today’s ranking.
- [ ] **DoD-4 — Oracle.** `normalize_summary_for_oracle` removes root `input_path_sort_order`. Allowlist does not contain that name. Unit test parent-without-key vs HEAD-with-paths.
- [ ] **DoD-5 — Tests.** Unit helper cases; keep-set two-file JSON/stderr; help asserts; oracle unit. Existing `source_rank_flips_winner_file_a_vs_a2` and `source_rank_ordered_primary_beats_dash2` still pass. Optional: `scan` two-file `--json` stderr does not contain the new `--source-rank` note.
- [ ] **DoD-6 — Recorded.** `review.md`; registry **Completed**; CHANGELOG CLI line; ledger **FEATURE**. Owner HITL optional; skip is allowed if recorded. No INC* PST in git.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test -p dedup-engine --lib source_rank
cargo test -p dedup-engine --lib multi_input_source_rank
cargo test -p pst-dedup-cli --test keep_set -- source_rank
cargo test -p pst-dedup-cli --lib oracle
cargo test -p pst-dedup-cli --test unique_pst_depth -- help
cargo test -p pst-dedup-cli --test unique_eml_depth -- help
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Full `cargo test --workspace` + `ledgerful verify` before commit. Do not run unique-pst against INC* in CI.

---

## 9. Deferred

| Row | Disposition |
|---|---|
| `D-0139-source-rank-discoverability` | **Absorb — this track.** Close on Completed. Do not change `first_seen`. |
| `D-0075-gui` | **Decline** — Desk ordered lists stay CLI-only. |
| Nested `Desktop\Desktop` path | **Decline** — operator FS; `\\?\` already works. |
| `D-0127-also-eml-dual-hint` | **Cite as precedent** (closed / 0127). Follow: parent emits, pack inner silent. |
| `D-0099-oracle-inputs-attest` | **Cite as constraint** (closed / 0102). Root-remove new path key; never allowlist `"inputs"` or this new name. |
| `D-0141-keepset-json-envelope` | **Leave** — 0141 owns stdout winner stripping. 0139 only requires the wrapper field survive. |
| `D-0145-scan-once-reuse` | **Leave** — note intent: reuse provenance order; do not implement `--from-scan-json` here. |
| `D-0108-*` / `D-0077-*` | **Decline** — CRC / poly / export_risk, not this track. |
| Bugbot #150–#153 | **Decline** — usage-limit only. |

---

## 10. Unblocks

Operators can see why `-2.pst` crowned and how to pass `--source-rank`. Parallel with **0140–0144** copy/JSON tracks. Does not unblock unique-pst wall time (**0146**).
