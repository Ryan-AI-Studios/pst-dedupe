# 0142 — DupsJsonTotals — Plan

> Map to `spec.md` §7. Execute in `C:\dev\Dedupe`. Status: **Completed** (PR **#160** / `a4dae1e`).
>
> **Ledger (execute):** `ledgerful ledger start 0142-dups-json-totals --category FEATURE --message "dups --json additive duplicates_total; --limit still caps sample"`
>
> **Fold-in 2026-09-25:** `agy-review.md` + `opencode-review.md` (bundle `AI-review.md`). Additive root totals; keep `duplicates` array; `scan --dups` parity; `--limit 0` → `duplicates_limit` null.

---

## Phase 0 — Re-verify → DoD-2, DoD-4 (precondition)

- [ ] Re-read `cmd_dups` JSON + `AlreadyEmitted` (~1940–1957) and `cmd_scan` JSON (~1798–1818).
- [ ] Confirm `args.limit == 0` → `dup_limit = None` on **both** commands.
- [ ] Confirm `collect_dups` cap + `ScanSummary.duplicates` == index `duplicate_count` (tier1+tier2 messages).
- [ ] Confirm `scan --json` without `--dups` emits `"duplicates": null`.
- [ ] Confirm `dups_json_failed_pst_single_document` and `scan_integrity` `dups --help` `--crc-log-limit` still exist.
- [ ] Confirm aspose single-file 0 dups; two-copy pattern in `keep_set.rs`.

---

## Phase 1 — Typed listing payload → DoD-1, DoD-2, DoD-3, DoD-4, DoD-5

- [ ] Add CLI-private `DupsJsonPayload` (name flexible) with frozen keys in spec §2.5. `duplicates_limit: Option<usize>` **without** `skip_serializing_if` (unlimited = JSON `null`). `csv` / `error` skip if none.
- [ ] Shared builder used by `cmd_dups` and `cmd_scan` **when listing** (`list_dups` / the `dups` command). `duplicates_total = outcome.summary.duplicates`. `duplicates_shown = dups.len()`. `duplicates_truncated = limit.is_some() && (shown as u64) < total`.
- [ ] `cmd_scan` without `--dups`: leave existing `{ ok, summary, csv, duplicates: null }` — do not attach listing fields.
- [ ] Do **not** edit `ScanSummary`, `DupRow`, `collect_dups` matching, or `dedup-engine`.
- [ ] `print_dups_text(dups, total)`: non-empty → `Duplicates ({shown} of {total} shown):`. Empty → `No duplicates listed.` Call from `cmd_dups` and `cmd_scan --dups`.
- [ ] clap `///` on `Dups` / `Scan` `--limit`: sample cap; `0` = unlimited; JSON reports `duplicates_total`. Do not change default 50.

---

## Phase 2 — Docs + help → DoD-6

- [ ] README (~65, ~72): `duplicates` is a capped sample; `duplicates_total` is corpus-wide (independent of `--limit`).
- [ ] CHANGELOG Unreleased: Changed (0142).
- [ ] `dups --help` / `scan --help`: `--limit` text matches §2.5. 0140 `--crc-log-limit` wording stays.

---

## Phase 3 — Tests → DoD-1 … DoD-5

Use `fixtures/aspose_outlook.pst`. No `include_str!`. Do not git-add INC* PSTs. New file `crates/pst-dedup-cli/tests/dups.rs` **or** extend `cli_matter.rs` (implementer choice; if no `dups.rs`, skip `--test dups` in §8).

**Migrate / extend:**

| Test | Change |
|---|---|
| `dups_json_failed_pst_single_document` | After `ok: false`, assert `duplicates_total`, `duplicates_shown`, `duplicates_limit`, `duplicates_truncated` exist (types/numbers; total may be 0). |
| `scan_integrity` `dups --help` `--crc-log-limit` | Unchanged pass. Optionally also require `--limit` help mentions sample / unlimited `0`. |

**New (two copies of aspose for a non-zero corpus, same as keep-set two-copy):**

- [ ] `--limit 1`: array length 1; `duplicates_total == summary.duplicates`; `duplicates_total > 1`; `duplicates_shown == 1`; `duplicates_limit == 1`; `duplicates_truncated == true`; `duplicates_total == summary.tier1_hits + summary.tier2_hits`.
- [ ] `--limit 0`: array length == `duplicates_total`; `duplicates_limit` is JSON `null`; `duplicates_truncated == false`.
- [ ] `--limit` ≥ total (e.g. 50 or 10_000): truncated false; shown == total; `duplicates_limit` is that number.
- [ ] Single aspose (0 dups): `duplicates == []`; total 0; shown 0; truncated false.
- [ ] `scan --json --dups --limit 1` on the two-copy corpus: same four fields and cap as `dups --json --limit 1`.
- [ ] `scan --json` **without** `--dups`: `"duplicates"` is `null`; four listing keys **absent**.
- [ ] Human: `dups` without `--json --limit 1` stdout contains `of` and the total (e.g. `1 of`).

Optional unit test next to `collect_dups` for truncated/`null` limit math (no PST).

---

## Phase N — Finalize → DoD-6

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Targeted tests in spec §8, then `cargo test --workspace`
- [ ] `ledgerful verify`
- [ ] `review.md`; registry **Completed**; ledger commit **FEATURE**
- [ ] Optional owner HITL on Desktop INC* `dups --json --limit 25` (`duplicates_total` 44, array 25) — skip allowed if recorded.

---

## Handoff notes

- Do not add a new root `schema`. `summary.schema` remains `scan_integrity_v1`.
- **0145** may later feed this listing from `--from-scan-json`; keep the helper free of a second scan walk, but do not implement reuse here.
- Unlimited `--limit 0` must serialize `duplicates_limit: null`, never `0`.
