# 0142 — `dups --json` totals vs `--limit` sample

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> **`--json` stays on stdout.** Logs stay on stderr.
> Distinct from **0141** (keep-set envelope) and **0145** (scan-once reuse).
> `--limit` still caps listed rows. Do not drop the cap. No BCC-default. Do not steal **0100–0104**. Not frontend.

- **Track ID:** 0142-DupsJsonTotals
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `dups --json` operator stdout. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-25).
- **Status:** Completed
- **Depends on:** `dups` / `scan --dups` CLI (shipped envelope `{ok, summary, duplicates}`)
- **Spec authored:** 2026-09-24 placeholder
- **Ready:** 2026-09-25 (coordinator fold of `agy-review.md` + `opencode-review.md`; placeholder expanded)
- **Fold-in:** 2026-09-25 `agy-review.md` + `opencode-review.md` (bundle `AI-review.md`)
- **Series:** X (INC0102784 CLI operator friction, readonly eval 2026-09-24)
- **Ledger category:** `FEATURE`

> **Closes / absorbs:** `D-0142-dups-json-totals`.
> **HITL (owner, not CI):** optional Desktop INC* `dups --json --limit 25` confirming `duplicates_total == 44` and `duplicates.len() == 25`. Never commit INC* PSTs.

---

## 1. Objective

`dups --json` (and `scan --json --dups`) must make corpus duplicate **counts** unambiguous when `--limit` truncates the sample array. Operators and agents must not read `duplicates.length` as the whole set.

Live output already has `summary` (`ScanSummary`) with corpus totals. The friction is a **naming/ambiguity** defect: top-level `"duplicates"` is a `--limit`-capped `Vec<DupRow>` while the true count sits in `summary.duplicates` with no root `duplicates_total` / `duplicates_shown` / truncation flag.

Deliver: **additive** root telemetry on the existing envelope. Keep `"duplicates"` as a capped array. Do not wrap it as `{ total, items }`. Do not invent a second summary object.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

INC* 2026-09-24: `dups --limit 25 --json` listed 25 of 44 Message-ID duplicate **messages**. Scripts that parse `payload.duplicates` conclude there are 25 duplicates. `summary.duplicates` already holds 44, but that key is easy to miss next to an array named `duplicates`.

This is JSON (and matching human-list) **honesty**, not a ranking, scan-once, or integrity-math change.

### 2.2 Live facts (plan-time HEAD post-0141 `74ae68f` / product squash `766f523`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| Schema | Matter **41**. N/A this track. No `SCHEMA_VERSION` bump. `ScanSummary.schema` stays `scan_integrity_v1`. |
| `cmd_dups` | `crates/pst-dedup-cli/src/main.rs` ~1875–1968. Cap: `dup_limit = None` iff `args.limit == 0`, else `Some(args.limit)` (~1930–1934). `collect_dups(&outcome, dup_limit)` (~1935). JSON: `serde_json::json!({ ok, summary: outcome.summary, duplicates: dups })` (~1940–1944). Integrity fail: print payload then `CliError::AlreadyEmitted` (~1945–1957). |
| `cmd_scan` | Same cap (~1785–1791). When `--json` **and** `--dups` (`list_dups`): `"duplicates"` is the capped array (~1804). When `--json` without `--dups`: `"duplicates": null`. Extra key `csv`. |
| `ScanSummary` | `scan.rs` ~157–214: `unique`, `duplicates` (from index `duplicate_count`), `tier1_hits`, `tier2_hits`, `opened_files`, … `summary.duplicates` is duplicate **messages** (`DedupIndex`: +1 per `DuplicateOf`, tier1 or tier2). |
| `collect_dups` | `scan.rs` ~2019–2040: one `DupRow` per `DuplicateOf`; breaks at `out.len() >= n`. `DupRow` is `Serialize`-only (~222–232). |
| clap | Workspace `"4"`. Lock **4.6.4**. crates.io / docs.rs latest **4.6.7**. `Dups.limit`: `#[arg(long, default_value_t = 50)] limit: usize` (`main.rs` ~176–177). No new flags. `--limit 0` stays unlimited. |
| serde | `Option<T>` without `skip_serializing_if` emits JSON `null`. Use that for `duplicates_limit` when unlimited. Do **not** emit `0` for unlimited. |
| Tests | Only `dups_json_failed_pst_single_document` (`tests/cli_matter.rs` ~634–644) exercises `dups --json` (fail shape). `scan_integrity.rs` asserts `dups --help` still documents `--crc-log-limit` (0140). aspose fixture: 17 msgs / 17 unique (0 dups on a **single** file). Two copies of aspose (keep-set `path_order_determinism_two_copies` pattern) produce full-file dups. |
| Human text | `print_dups_text` (`main.rs` ~2037–2042): `Duplicates ({} shown):` uses sample length. `print_summary_text` already prints `duplicates:    {}` from `summary.duplicates`. Used by `cmd_dups` and `cmd_scan --dups`. |
| 0141 pattern | CLI-private typed envelope (`KeepSetEnvelope` / `KeepSetSummaryOut` in `keep_set_cmd.rs`). Prefer a typed `DupsJsonPayload` over another ad-hoc `json!` map. |

### 2.3 Tools (fold-in 2026-09-25)

| Tool | Result |
|---|---|
| `ai-brains preflight --summary` | Vault live (project `93e74c21`). 5020 pins. Recall: Series X 0142 owns totals vs `--limit`; no BCC. |
| `ledgerful doctor --json` | `readyForPublish: true`, 0 block. Unrelated warn: phantom verify rows, completion-model cold. |
| `ledgerful ledger status --compact` | 0 pending, 0 unaudited drift. |
| Last-PR Cursor | agy: PRs **#156–#159** Bugbot usage-limit only. No 0142-owned inline finding. **Decline mint.** |
| clap | No new flags. Existing `--limit` `usize` + `default_value_t = 50` is clap 4 derive. |

### 2.4 Product locks

- Do not change default `--limit` (50) or `--limit 0` = unlimited.
- Do not rename or retype top-level `"duplicates"` (stays `Vec<DupRow>` when listing).
- Do not edit `ScanSummary`, `DupRow` fields, `collect_dups` cap logic, or `dedup-engine` index/counts.
- Do not implement scan-once reuse (**0145**). Totals helper should be reusable later; do not add `--from-scan-json`.
- Do not touch keep-set envelope (**0141**), poly CRC copy (**0143**), integrity CSV (**0144**), unique-pst write (**0146**), deep-attach (**0147**).
- Do not bump matter schema 41. No BCC-default.
- Do not move logs onto stdout. Do not add `--progress-file`.
- Do not disturb 0140 `--crc-log-limit` / folder cadence help or behavior.

### 2.5 JSON contract (locked)

CLI-private typed payload in `pst-dedup-cli` (`main.rs` and/or `scan.rs` helper). **Additive.** Shared by `cmd_dups` and `cmd_scan` when a duplicates **array** is emitted.

**When listing duplicates** (`dups --json`, or `scan --json --dups`):

| Field | Type / rule |
|---|---|
| `ok` | Unchanged. |
| `summary` | Unchanged `ScanSummary`. Do not duplicate its fields into a second block. |
| `duplicates` | **Array** of `DupRow`, still capped by `--limit`. Never an object `{ total, items }`. |
| `duplicates_total` | `u64`. **Must** equal `summary.duplicates` (`tier1_hits + tier2_hits` / index `duplicate_count`). Duplicate **messages**, not groups or pairs. Do **not** key off `tier1_hits` alone. |
| `duplicates_shown` | `usize` = `duplicates.len()`. |
| `duplicates_limit` | `Option<usize>` **always present**. JSON **number** when `--limit > 0`. JSON **`null`** when `--limit 0` (unlimited). Never `0` meaning unlimited. Do **not** `skip_serializing_if`. |
| `duplicates_truncated` | `bool`. `true` iff a cap is in effect **and** `duplicates_shown < duplicates_total`. Else `false`. |
| `error` | Unchanged; present on integrity failure. |
| `csv` | `scan` only; omit on `dups` (`skip_serializing_if` none). |

**`scan --json` without `--dups`:** keep `"duplicates": null`. Do **not** add the four listing fields on that path (`summary.duplicates` already exists).

**`--limit` boundaries:**

| Case | `duplicates` | `duplicates_total` | `duplicates_shown` | `duplicates_limit` | `duplicates_truncated` |
|---|---|---|---|---|---|
| `--limit N` and `N < total` | length `N` | corpus | `N` | `N` | `true` |
| `--limit N` and `N >= total` | length `total` | corpus | `total` | `N` | `false` |
| `--limit 0` (unlimited) | length `total` | corpus | `total` | `null` | `false` |
| zero dups | `[]` | `0` | `0` | as above | `false` |

**Integrity failure:** same four fields on the printed payload before `AlreadyEmitted`. Totals may be 0 on a non-PST; keys must still exist.

**Human text (in scope):** `print_dups_text` for non-empty lists: `Duplicates ({shown} of {total} shown):`. Empty stays `No duplicates listed.` `cmd_dups` and `cmd_scan --dups` both pass `summary.duplicates`.

---

## 3. In scope

1. Additive root fields on `dups --json` and `scan --json --dups` via one typed helper/struct.
2. Keep `"duplicates"` as a capped `Vec<DupRow>`.
3. `--limit 0` / zero-dup / `limit >= total` contracts in §2.5.
4. Failure-envelope totals (`AlreadyEmitted` path).
5. Human list line `N of M shown` (both commands).
6. Tests in §7 / plan Phase 3; `dups`/`scan --dups` `--help` `--limit` wording; README + CHANGELOG Unreleased.

---

## 4. Out of scope (do NOT do here)

- Changing default `--limit` (50) or unlimited-`0` semantics.
- Embedding full dup rows without limit.
- Scan-once reuse / `--from-scan-json` (**0145**). Streaming `collect_dups` without `retain_rows`.
- `scan --json` without `--dups` growing listing fields.
- Renaming `"duplicates"` → `"duplicates_sample"`.
- Keep-set JSON (**0141**), poly CRC copy (**0143**), integrity CSV (**0144**), unique-pst write hotspots (**0146**), deep-attach coverage (**0147**).
- Editing `dedup-engine` / `ScanSummary` / `DupRow` / `collect_dups` matching rules.
- Matter `SCHEMA_VERSION`. BCC-default. A new top-level `schema` string (summary already has `scan_integrity_v1`).

---

## 5. Preconditions & dependencies

- **P1:** `dups` / `scan --dups` JSON envelope exists (shipped).
- **P2:** 0140 `dups --help` `--crc-log-limit` test must still pass.
- *Verified to date:* envelope already has `summary`; `--limit 0` → `None`; aspose single-file 0 dups; two-copy aspose yields dups; clap 4.6.4.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Reshape `"duplicates"` into an object | Frozen array + additive keys only |
| `duplicates_total = tier1_hits` undercount | Equals `summary.duplicates`; assert `== tier1_hits + tier2_hits` |
| `duplicates_limit: 0` read as “allow none” | Unlimited → JSON `null` |
| Patch `cmd_dups` only | Shared helper; `scan --dups` in DoD |
| Ad-hoc `json!` typo (`duplicate_total`) | Typed `DupsJsonPayload` |
| Break 0140 help test | Do not rewrite `--crc-log-limit` help |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Additive totals:** `dups --json --limit N` (N > 0, N < corpus dups) has `duplicates` array length `N`, `duplicates_total` equal to `summary.duplicates` and greater than `N`, `duplicates_shown == N`, `duplicates_limit == N`, `duplicates_truncated == true`. `summary` still present and unchanged in role. No second summary object.
- [ ] **DoD-2 — Cap retained:** `duplicates` remains a JSON array of `DupRow`. `duplicates.len() <= limit` when limit > 0. Default `--limit` stays 50.
- [ ] **DoD-3 — Boundaries:** `--limit 0` → `duplicates_limit` is JSON `null`, `duplicates_shown == duplicates_total`, `duplicates_truncated == false`. Zero-dup fixture (single aspose): `duplicates == []`, totals 0, truncated false. `limit >= total` → truncated false.
- [ ] **DoD-4 — `scan --dups` parity:** `scan --json --dups --limit N` emits the same four listing fields and the same cap behavior. `scan --json` without `--dups` still has `"duplicates": null` and does **not** add those four fields.
- [ ] **DoD-5 — Failure + human:** `dups --json` integrity-fail payload still includes the four fields. Human list prints `Duplicates (N of M shown):` when non-empty (`cmd_dups` and `scan --dups`).
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; ledger **FEATURE**; README `dups` / `scan --dups` sentences; CHANGELOG Unreleased. Optional INC* HITL skip allowed if recorded.

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p pst-dedup-cli --test cli_matter --test scan_integrity
cargo test -p pst-dedup-cli --test dups
cargo test --workspace
ledgerful verify
```

If tests live only in `cli_matter.rs` (no `tests/dups.rs`), run that file instead of `--test dups`.

---

## 9. Deferred

| ID | Disposition |
|---|---|
| `D-0142-dups-json-totals` | **Absorb** — this track. Wording: capped `duplicates` array reads as the whole set; add explicit root totals. |
| `D-0141-keepset-json-envelope` | Closed / 0141. Typed-envelope pattern only. |
| `D-0139` / `D-0140` | Closed. Preserve 0140 `--crc-log-limit` help. |
| `D-0143` / `D-0144` | Decline here — siblings. |
| `D-0145-scan-once-reuse` | Decline here — sibling. Totals helper should be reusable there later. |
| `D-0146` / `D-0147` | Decline here — siblings. |

No related open deferred row skipped. `docs/deferred.md` row stays “Owned by 0142” until the implement docs PR closes it (this fold does not edit `docs/`).

### Fold-in declines / corrections

| Id | Disposition |
|---|---|
| AGY-142-01 / OC-142-01 unexpanded placeholder | **Agree — fold** — this expansion. |
| AGY-142-02 / OC-142-02 stale “add envelope” | **Agree — fold** — de-ambiguate; keep existing `summary`. |
| AGY-142-03 / OC-142-03 freeze array + additive keys | **Agree — fold** — names in §2.5. |
| OC-142-03 `limit_applied` | **Agree — partial** — use `duplicates_limit` (`null` if unlimited) + `duplicates_truncated`. Not `limit_applied`. |
| AGY-142-04 / OC-142-04 total = duplicate messages | **Agree — fold** — `== summary.duplicates`, not `tier1_hits` alone. |
| AGY-142-05 / OC-142-06 `scan --dups` | **Agree — fold** — in scope; share helper. `scan --json` without `--dups` stays `null`. |
| AGY-142-06 / OC-142-05 `--limit 0` / zero / `limit > total` | **Agree — fold** — §2.5 table. |
| AGY-142-07 / OC-142-08 test inventory | **Agree — fold** — plan Phase 3. |
| AGY-142-08 / OC-142-07 human text | **Agree — fold** — `N of M shown` in scope. |
| AGY-142-09 / OC-142-09 `AlreadyEmitted` | **Agree — fold** — DoD-5. |
| AGY-142-10 typed struct | **Agree — fold** — `DupsJsonPayload` (name implementer-local). |
| AGY-142-11 / OC-142-10 README + CHANGELOG | **Agree — fold** — DoD-6. |
| OC-142-11 freeze names | **Agree — fold** — §2.5. |
| AGY-142-12 / OC-142-12 retain_rows / engine | **Already covered** — OOS 0145 / engine fence. |
