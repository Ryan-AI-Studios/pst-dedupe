# 0141 — Keep-set JSON envelope (stats vs winners)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> **`--json` stays on stdout.** Logs stay on stderr.
> Distinct from **0139** (ranking / `first_seen` / `input_path_sort_order`) and **0145** (scan-once reuse).
> Sidecar `--keep-set-json` stays full `keep_set_v1`. No BCC-default. Do not steal **0100–0104**. Not frontend.

- **Track ID:** 0141-KeepSetJsonEnvelope
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** keep-set `--json` operator stdout. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-25).
- **Status:** Completed
- **Depends on:** **0066** keep-set `--json` · **0078** self-locating `keep_set_summary.json` · **0139** Completed (`input_path_sort_order`)
- **Spec authored:** 2026-09-24 placeholder
- **Ready:** 2026-09-25 (coordinator fold of `agy-review.md` + `opencode-review.md`; placeholder expanded)
- **Fold-in:** 2026-09-25 `agy-review.md` + `opencode-review.md` (bundle `AI-review.md`)
- **Series:** X (INC0102784 CLI operator friction, readonly eval 2026-09-24)
- **Ledger category:** `FEATURE`

> **Closes / absorbs:** `D-0141-keepset-json-envelope`.
> **HITL (owner, not CI):** optional Desktop INC* `keep-set --json` confirming stdout is kilobytes, sidecar still ~4055 winners. Never commit INC* PSTs.

---

## 1. Objective

`keep-set --json` stdout must be an **agent-sized envelope** (stats, fidelity, paths, `exit_code`) unless the operator opts into inline winners. INC* 2026-09-24: stdout **3.4 MB** duplicated `--keep-set-json` (**3.3 MB**, 4055 winners).

Deliver: default `--json` omits `keep_set.winners`; `--include-winners` restores them on **stdout only**; `--keep-set-json` remains the full `keep_set_v1` document. Ranking, scan-once reuse, unique-pst/unique-eml summaries, and matter schema stay as shipped.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

`KeepSetSummaryOut` embeds `pub keep_set: dedup_engine::KeepSet`, whose `winners: Vec<KeepEntry>` is the bulk of INC* JSON. `run_keep_set` builds `summary_value` **once** and reuses it for on-disk `keep_set_summary.json` (0078 DoD-22) **and** pretty-printed stdout under `--json`. Agents that ask for `--json` currently ingest the entire winner list even when `--keep-set-json` already wrote it.

This is stdout/summary **size honesty**, not a keep-set policy change and not a `keep_set_v1` sidecar bump.

### 2.2 Live facts (plan-time HEAD `c6f6ff3`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| Schema | Matter **41**. N/A this track. No `SCHEMA_VERSION` bump. Sidecar `keep_set_v1` unchanged. |
| `KeepSetSummaryOut` | `crates/pst-dedup-cli/src/keep_set_cmd.rs` ~140–165. Top-level `schema` is cloned from `keep_set.schema` → `"keep_set_v1"`. Fields: `input_path_sort_order` (0139), `policy`, `family_policy`, `keep_set`, `scan`, `decision_csv`, `keep_set_json`, `materialized`, `ok`, `fidelity`, `exit_code`, `exit_reason`, `artifact_state`, `summary_path`, `error`. |
| `KeepSet` | `crates/dedup-engine/src/keepset.rs` ~603–617: `schema`, `policy`, `family_policy`, `created_from`, `identity_level`, `dedupe_scope`, **`winners`**, `stats`. Already uses `skip_serializing_if` on optional provenance fields. **Do not modify this struct.** |
| Dual emit | `build_summary` ~329–356; `summary_value` ~358; disk write ~361; rewrite on write-fail ~386–388; stdout `to_string_pretty` ~391–392. `AlreadyEmitted` ~403 and ~475. |
| Sidecar | `write_keep_set_json` (`keepset.rs` ~3033): bare `KeepSet` with top-level `winners`. unique-eml/unique-pst tests read **that file**, not keep-set stdout. |
| Unique-* summaries | `UniqueExportSummary.keep_set: dedup_engine::KeepSet` (`unique_export_report.rs` ~928). `export_oracle.rs` normalizes `/keep_set/winners`. **Out of scope.** |
| clap | Workspace `"4"`. Lock **4.6.4**. docs.rs latest **4.6.7**. `#[arg(long)] json: bool` is clap 4 derive `SetTrue`. Same for new `--include-winners`. Do **not** add a value-taking flag. |
| serde | `skip_serializing_if = "Option::is_none"` already in this crate. Use it so omitted winners are a **missing key**, not `[]`. |
| CLI tests | `crates/pst-dedup-cli/tests/keep_set.rs`. Stdout winner-array parses live in **four** fns (opencode inventory; agy names were stale): `keep_set_json_schema_and_decision_csv_header` (~74), `path_order_determinism_two_copies` (~171), `source_rank_flips_winner_file_a_vs_a2` (~554, 583), `aspose_default_winners_deterministic_golden` (~1163). `keep_set_input_flag_works` asserts top-level `schema == keep_set_v1` (~128) with no winner parse. `export_exit_0078.rs` asserts `exit_code` / `fidelity` / `summary_path` match disk; does **not** parse winners. |
| Fixture | `fixtures/aspose_outlook.pst` (keep-set / 0078 already). |
| `--json` | stdout envelope only; logs already on stderr (CLI `long_about`). |

### 2.3 Tools (fold-in 2026-09-25)

| Tool | Result |
|---|---|
| `ai-brains preflight --summary` | Vault live (project `93e74c21`). 5017 pins. Series X placeholders still the last mint decision. |
| `ai-brains recall` | 0066 `keep_set_v1` contract; 0108 keep-set not restripped; Series X 0141 owns JSON envelope. |
| `ledgerful doctor --json` | `readyForPublish: true`, 0 block. Unrelated warn: phantom verify rows, sig pin/version. |
| `ledgerful ledger status --compact` | 0 pending, 0 unaudited drift. |
| Last-PR Cursor | agy: PRs **#154–#157** Bugbot usage-limit only. No 0141-owned inline finding. **Decline.** |

### 2.4 Product locks

- Do not change `first_seen` / `--source-rank` ranking (**0139**). Keep `input_path_sort_order`.
- Do not bump sidecar `keep_set_v1` or edit `dedup_engine::KeepSet`.
- Do not change unique-pst / unique-eml JSON summaries or `export_oracle.rs`.
- Do not implement scan-once reuse (**0145**).
- Do not restrip CRC-tainted keep-set winners (**D-0108-keepset-crc-retaint**).
- Do not touch `dups --json` (**0142**), poly CRC copy (**0143**), or integrity CSV (**0144**).
- Do not move logs onto stdout. Do not add `--progress-file`.
- Do not disturb 0140 `--crc-log-limit` / folder cadence on keep-set.

### 2.5 Envelope contract (locked)

CLI-private types in `keep_set_cmd.rs` (wrapper / `serialize_with` / sibling struct — implementer choice). **Not** a change to `dedup_engine::KeepSet`.

**Top-level envelope** (`KeepSetSummaryOut` or rename in-file):

| Field | Rule |
|---|---|
| `schema` | **`keep_set_summary_v1`** (new). Today this is copied from `keep_set.schema` (`keep_set_v1`) — dishonest once winners are omitted. |
| `winners_inline` | `bool`. `false` on disk always. `true` on stdout only when `--json --include-winners`. |
| `input_path_sort_order` | Unchanged 0139. |
| `policy`, `family_policy`, `scan`, `decision_csv`, `keep_set_json`, `materialized`, `ok`, `fidelity`, `exit_code`, `exit_reason`, `artifact_state`, `summary_path`, `error` | Retain. |
| `keep_set` | Nested object, inner `schema` remains **`keep_set_v1`**. Must still serialize `policy`, `family_policy`, `created_from`, `identity_level`, `dedupe_scope`, `stats`. |

**Nested `keep_set.winners`:**

| Mode | JSON |
|---|---|
| Default envelope (stdout and `keep_set_summary.json`) | **Key absent** (`Option::None` + `skip_serializing_if`). Never `[]`. `winners_inline: false`. |
| `--json --include-winners` stdout | Array of `KeepEntry`, **order-identical** to `--keep-set-json` sidecar `winners`. `winners_inline: true`. |
| `--keep-set-json` file | Unchanged full `keep_set_v1` (top-level `winners` array). |

**`--include-winners` without `--json`:** no-op (human summary already has no winner list). Do not write winners into `keep_set_summary.json`.

**Disk vs stdout:** `keep_set_summary.json` is **always** the agent-sized envelope (no winners), including when `--include-winners` is set. Rebuild or overlay winners **only** for the stdout `println` when `--json --include-winners`. Thread the envelope through both `build_summary` call sites (success and 0078 write-failure rewrite). `AlreadyEmitted` still prints the envelope first.

**False-zero:** consumers must not treat a missing `winners` key as zero uniques. `keep_set.stats.unique` stays populated. `winners_inline` distinguishes policy-omit from an empty keep-set.

---

## 3. In scope

1. CLI-private keep-set JSON envelope in `keep_set_cmd.rs`.
2. clap `--include-winners` on `keep-set` only; thread through `KeepSetCliArgs` / `main.rs`.
3. Default `--json` omits nested winners; opt-in restores stdout winners.
4. `keep_set_summary.json` always envelope-sized.
5. Test migrations in `tests/keep_set.rs` + assertions in `tests/export_exit_0078.rs` that disk/stdout still share `exit_code` / `fidelity` / `summary_path`.
6. `keep-set --help`, README keep-set `--json` sentence, CHANGELOG Unreleased.

---

## 4. Out of scope (do NOT do here)

- Changing keep-set policy / `first_seen` / `--source-rank` (**0139**).
- Scan-once reuse (**0145**).
- Schema bump of sidecar `keep_set_v1` (unless a compatible additive field — **not needed**).
- `unique-pst` / `unique-eml` summaries, `UniqueExportSummary.keep_set`, `export_oracle.rs`.
- `dups --json` totals (**0142**).
- Poly CRC copy (**0143**), integrity CSV (**0144**), unique-pst write hotspots (**0146**), deep-attach coverage (**0147**).
- Editing `dedup-engine` keep-set resolve / `KeepSet` / `KeepEntry`.
- Matter `SCHEMA_VERSION`.
- BCC-default.

---

## 5. Preconditions & dependencies

- **P1:** 0066 keep-set CLI and 0078 summary path exist (shipped).
- **P2:** 0139 `input_path_sort_order` must survive the envelope refactor.
- *Verified to date:* dual emit of one `summary_value`; four stdout winner parsers in `keep_set.rs`; sidecar consumers in unique-eml/unique-pst; clap 4.6.4 `SetTrue` bool flags.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Implementer edits `dedup_engine::KeepSet` | Spec fence + unique-* OOS; CLI-private type only |
| `winners: []` false-zero | `Option` + skip; `winners_inline` |
| Disk still 3.4 MB | Disk always envelope; winners only stdout opt-in + sidecar |
| Top-level `schema` still `keep_set_v1` | Rename envelope to `keep_set_summary_v1`; migrate stdout asserts; sidecar stays `keep_set_v1` |
| Dropping 0139/0078 fields | Field retain list in §2.5; 0078 tests keep `exit_code`/`fidelity`/`summary_path` |
| `--include-winners` without `--json` surprises | Documented no-op |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Default envelope:** `keep-set --json` (with or without `--keep-set-json`) omits `keep_set.winners` (key absent). Envelope has `schema == keep_set_summary_v1`, `winners_inline == false`, populated `keep_set.stats`, `input_path_sort_order`, `ok` / `fidelity` / `exit_code` / `summary_path`. Fixture stdout **< 16 KiB**.
- [ ] **DoD-2 — `--include-winners`:** Frozen name. With `--json`, stdout includes `keep_set.winners` as an array, `winners_inline == true`, order-identical to `--keep-set-json` sidecar `winners`. Without `--json`, no-op. clap `///` says it restores inline winners under `--json`.
- [ ] **DoD-3 — Tests migrated:** Live stdout winner parsers updated per plan Phase 3. Sidecar `keep_set_v1` tests still see winners. New tests: omit + size; include restores; disk omits even with `--include-winners`; `--help` lists `--include-winners`. `export_exit_0078` keep-set tests still match `exit_code` / `fidelity` / `summary_path`.
- [ ] **DoD-4 — Disk contract:** `keep_set_summary.json` is the envelope (no winners) on every keep-set run, including `--include-winners` and stdout-only `--json`.
- [ ] **DoD-5 — Isolation / fence:** `--json` remains one stdout value. unique-pst / unique-eml summaries unchanged. `dedup_engine::KeepSet` unmodified. 0139 `input_path_sort_order` and 0140 keep-set `--crc-log-limit` help remain.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; ledger **FEATURE**; README + CHANGELOG. Optional INC* HITL skip allowed if recorded.

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p pst-dedup-cli --test keep_set --test export_exit_0078
cargo test --workspace
ledgerful verify
```

---

## 9. Deferred

| ID | Disposition |
|---|---|
| `D-0141-keepset-json-envelope` | **Absorb** — this track. |
| `D-0139-source-rank-discoverability` | Closed / 0139. Preserve `input_path_sort_order`. |
| `D-0140-scan-stderr-cadence` | Closed / 0140. Do not disturb keep-set cadence / `--crc-log-limit`. |
| `D-0142-dups-json-totals` | Decline here — sibling. |
| `D-0143` / `D-0144` | Decline here — siblings. No keep-set restrip (`D-0108-keepset-crc-retaint`). |
| `D-0145-scan-once-reuse` | Decline here — sibling. |

No related open deferred row skipped.

### Fold-in declines / corrections

| Id | Disposition |
|---|---|
| AGY-141-04 test names `prefer_path_contains_wins_over_first_seen` / `source_rank_ordered_primary_beats_dash2` / `keep_set_winners_match_checked_in_fixture` | **Decline names** — those fns are not in live `keep_set.rs`. Use OC-141-06 inventory. |
| AGY-141-09 10 KiB bound | **Agree — partial** — CI bound is **16 KiB** on aspose (scan blob headroom). Omit + `winners_inline` remain required. |
| AGY-141-05 winners on envelope root vs nested | **Agree — partial** — omit nested `keep_set.winners`; `winners_inline` is **top-level**. Do not add a second root `winners` array. |
