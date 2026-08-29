# 0109 — unique-pst `--also-eml` classify / summary honesty

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open 0107 co-export wiring, 0108
> poly degrade, keep-set ranking, HNBITMAPHDR, BCC default, or frontend (0110+).

- **Track ID:** 0109-AlsoEmlClassifyHonesty
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `docs/unique-pst-export.md` + this track. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-29); do **not** chase it at execute.
- **Cross-repo contract:** n/a
- **Status:** In progress
- **Depends on:** 0107 (Completed — `--also-eml` co-export; PR **#104** / `339dfa0`)
- **Spec authored:** 2026-08-29 (placeholder → Ready)
- **Series:** S (Unique-export HITL residuals, post-0107)
>
> **Closes:** `D-0109-also-eml-classify` (PR #104 Cursor Bugbot, three findings still live on `main`).
> **HITL:** none required. Classify/cancel honesty is CI-testable. Optional operator re-smoke of INC* is not a gate.
>
> **Last-PR fold-in (2026-08-29):** PRs **#108, #107, #106, #105**. Origin Bugbot is **#104**. Disposition in §2.8.
>
> **Review fold-in (2026-08-29):** `opencode-review.md` + `agy-review.md`. Disposition in §2.9 and `foldin-note.md`. Locks: `finalize_unique_pst_classify` is **fidelity worse-of only** (exit/reason merge stays at live combined-exit block); recovery test seeds `summary.json` as a **file** and blocks `manifest.json` as a **directory** with `cancel=true`; attach/embedded recover from JSON only; allow-partial summaries emit `error.code=partial_fidelity`.
>
> **Not frontend.** Series O is **0110+**. **0108** owns poly `degraded_winner_rate` (Completed). No BCC-default track.

---

## 1. Objective

Keep combined unique-pst + `--also-eml` **0078 classify honest** after 0107. Three Cursor Bugbot findings on merged PR **#104** are still live on `main` @ `f49857e` (re-verify line numbers at execute):

1. Combined classify derives `fidelity` from the merged **exit** and sets `ok` from exit `0`. `--allow-partial-fidelity` then becomes `fidelity=complete` / `ok: true` when also-eml ran. Risk-gate exit **65** is mapped to `failed`. The same `ok = (exit == 0)` assignment also fires **without** `--also-eml`.
2. After an also-eml cancel, a failed unique-pst `summary.json` rewrite reclassifies with the **PST-only** `cancelled` flag. Combined **130** becomes generic **1**, and `retryable` is recomputed as a permanent report failure.
3. When pack write returns `Err` after cancel, the helper converts that to a cancelled `Ok` but fills `attach_parts_failed` and `embedded_messages_written` with **zeros** instead of recovering them from the on-disk summary. unique-pst copies those zeros into `also_eml_*`.

This track restores the 0078 contract: **`ok == (fidelity == complete)`**, fidelity comes from classified outcomes (not from exit integers), cancel stays **130** / cancel-retry, and also-eml counters stay recovered from disk. Combined process exit stays 0078 precedence `130 > 1 > 65 > 64 > 0` (not raw `u8` max).

This advances unique-export **defensibility**: counsel and automation must not see a partial-allowed or cancelled also-eml job as complete success.

---

## 2. Context (read before starting)

### 2.1 Diagnosis (plan-time 2026-08-29; re-verify line numbers at execute)

HEAD `f49857e` (post-0108 `cf655a6` / PR #106; 0107 `339dfa0` / PR #104).

| Surface | Live state |
|---|---|
| `unique_pst_cmd.rs` ~3340–3346 | PST `classify_export` is correct (0078). |
| ~3352–3383 | Combined **exit** uses `worse_cli_exit` + `also_eml_cancelled` → 130. **Keep.** |
| ~3416–3427 | **Bug 1.** If `also_eml_ran \|\| also_eml_cancelled`, `fidelity` is rewritten from `combined_exit`: Success→Complete, PartialFidelity→Partial, else Failed. Then `ok = combined_exit == Success && !process_cancelled`. |
| `docs/unique-pst-export.md` ~626 | Contract: `ok == (fidelity == complete)`; `--allow-partial-fidelity` → exit **0**, JSON still `fidelity: partial`. |
| `export_outcome.rs` ~528–535 | `allow_partial_fidelity_exit_0`: fidelity **Partial**, exit **Success**. Risk-gate 65 keeps fidelity **Complete** (~548–559). |
| `unique_eml_cmd.rs` ~740 | Standalone unique-eml already uses `ok = fidelity == Complete && !cancelled`. unique-pst diverged. |
| `unique_pst_cmd.rs` ~3536–3577 | **Bug 2.** Summary-write failure re-calls `classify_export(..., cancelled)` with the **PST** flag, then `summary_is_retryable(..., cancelled, ...)`. Also-eml cancel is `also_eml_cancelled` / `process_cancelled` (~3359). |
| JSON return ~3649–3656 | Process exit is `classified.exit`. Rewrite that drops 130 → 1 changes the shell code. |
| `unique_eml_cmd.rs` ~449–471 | **Bug 3.** Cancel `Err` → `Ok` cancelled result: `eml_written` from `count_eml_under`; `attach_parts_failed: 0`; `embedded_messages_written: 0`. |
| `also_eml_recovered_counts` ~222–261 | Recovers attach/embedded from `summary.json` or `manifest.json`. unique-pst **Err** arm already uses it (~3170–3174, ~3202–3206). Cancel **Ok** conversion does not. |
| `WriteEmlPackFromKeepSetResult` ~181–191 | Has `exit` / `cancelled` / counts. **No** `fidelity` field — inner already classifies (~733–739). |
| `export_exit_0078.rs` ~138–161 | Allow-partial `ok: false` is **aspose-dependent** (`attachments_failed > 0`). Clean fixtures skip the assertion — CI did not catch 0107’s `ok` rewrite. |
| `unique_pst_also_eml.rs` `helper_cancel_with_blocked_summary_returns_cancelled_ok` | Asserts 130 / CANCELLED; does **not** assert attach/embedded counts (summary.json is a **directory** so there is nothing to recover). |

**Do not** treat 0107 co-export wiring as open. Pack write, path guards, combined exit merge, skip-also-eml-on-PST-cancel, and `also_eml_*` keys shipped.

### 2.2 Why fidelity must not be derived from exit

0078 `classify_export` maps **Partial + `--allow-partial-fidelity`** to exit **0** while fidelity stays `partial`. Risk gate maps **Complete** to exit **65**. Those two exits are not a fidelity enum.

0107’s rewrite `Success → complete` therefore:

| Job | Honest 0078 | Live with `--also-eml` |
|---|---|---|
| PST partial, allow-partial, EML complete | `fidelity=partial`, `ok=false`, exit **0** | `complete` / `ok=true` / 0 |
| PST complete, `--fail-on-export-risk`, also-eml complete | `fidelity=complete`, `ok=true`, exit **65** | `failed` / `ok=false` / 65 |
| PST complete, also-eml attach soft-fail, default fail-on-partial | combined exit **64**, combined fidelity **partial** | 64 / `partial` / `ok=false` (exit 64 is already not Success; the lie here is **fidelity-from-exit**, which happens to match Partial for 64) |

The same `ok = (exit == 0)` line runs **without** `--also-eml`, so unique-pst allow-partial also reports `ok: true` whenever the process is 0. Restore `ok == (fidelity == complete) && !cancelled` for **all** unique-pst jobs.

### 2.3 Combined fidelity vs PST `artifact_state`

`artifact_state` is the disposition of unique-pst **`--out`** (PST volumes + PST quarantine). 0107 lock: also-eml cancel quarantines the EML dir only; **PST is kept**.

| Field | After 0109 |
|---|---|
| `fidelity` / `ok` / `exit_code` | **Combined job** (worse of PST + EML classified outcomes; 0078 exit precedence). |
| `artifact_state` | **PST `--out` only** — computed from PST `classify_export` + PST quarantine **before** the also-eml merge. Do **not** recompute from combined Failed after also-eml-only cancel (that would mark a complete PST `invalid_in_place`). |
| `also_eml_exit_code` / `{also_eml}/summary.json` | EML pack’s own 0078 fields. |

Document this split in `docs/unique-pst-export.md`. Do **not** add `also_eml_fidelity` (pack summary already has `fidelity`). Schema id `unique_export_report_v1` **not** bumped.

### 2.4 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3882 pinned).
- `ai-brains sync query` / lexical `recall` — 0107 Completed PR #104; combined exit 0078 precedence (not raw `max(u8)`); frontend **0110+**. Semantic recall timed out; lexical used.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` **0 pending / 0 unaudited drift**. `scan --impact` **LOW** (HEAD `f49857e`; dirty tree is `.claude` junction + `agy-review.md` + `fixtures/keep_set_summary.json`; `conductor/` gitignored). Soak `output/inc0102784-post-0107/` blew the 5k file budget — do not commit it. Hotspot `export_exit_0078.rs` is in scope for **tests**. `unique_wizard.rs` hotspot is **out of scope** (`also_eml: None`). Doctor: embed unreachable, phantom-promote, sig-pin — none block planning.
- Ledger tx for this planning pass: `3db3c68e-e19b-4ade-8838-cd8b5252aae6`.

### 2.8 Last-PR Cursor comments (mandatory)

| PR | Surface | Disposition |
|---|---|---|
| **#104** | 3 Cursor Bugbot: (M) partial marked complete + `ok` from exit 0, including without `--also-eml`, and risk 65→failed; (M) summary rewrite drops also-eml cancel 130; (L) cancel recovery zeros attach/embedded | **Absorb all three here** (this track already owned them as a placeholder). |
| **#106** | 0108 poly effective degrade | **none** on classify/cancel/also-eml. Do not steal 0108. |
| **#108, #107, #105** | chore ignore `.agents/`; docs merge-SHA | **none** |

No new placeholder. Next free ID remains **0117**. No BCC-default track.

### 2.9 Review fold-in (2026-08-29)

| Id | Disposition |
|---|---|
| opencode-m1 | **Agree — fold** — `finalize_unique_pst_classify` is **fidelity worse-of + call-site `ok` only**. Signature: `(pst: ExportOutcome, eml_fidelity: Option<ExportFidelity>)`. Exit/reason merge **stays** at live `worse_cli_exit` / also-eml-cancel 130 (~3353–3415), including eml-worse-first reason order and `String → &'static` remap. Helper must **not** take `exit`/`reasons` (avoids double-merge and unused-field clippy). |
| opencode-m2 | **Agree — fold** — recovery test trigger pinned in §3.5 / plan Phase 3: seed `summary.json` as a **file** (counts 7/2/3) **and** make `manifest.json` a **directory** (same Err as `helper_hard_fail_writes_summary_json`) **and** `cancel=true` so Err→Ok. Do not block `summary.json` as a directory. |
| opencode-o1 | **Agree — fold** — DoD-3 / §3.4: attach/embedded recover from summary/manifest **JSON only**; `eml_written` may fall back to counting `.eml` files. Attach/embedded may still be 0 when only `.eml` files exist. |
| opencode-o2 | **Agree — fold** — §3.6: after `ok=false` on allow-partial, summary `error.code` is `partial_fidelity` (retryable stays false). Matches unique-eml. |
| opencode-o3 | **Agree — fold** — §2.2 row 3 parenthetical corrected (exit 64 was never `ok=true`). |
| opencode-o4 | **Already covered** — Phase 0 re-verifies line numbers. |
| agy-F-0109-1 / F-0109-2 / F-0109-3 | **Already covered** — DoD-1 / DoD-2 / DoD-3 (the three Bugbot sites). |
| agy-F-0109-4 | **Already covered** — §2.3 `artifact_state` pre-merge lock. |
| agy false-pass / pre-seed | **Agree — fold** with m1/m2 — §3.5 constructs `ExportFidelity::Partial` on the helper (no aspose attach-fails); recovery seeds **non-zero** JSON. |

agy named `CliExit::RiskGate` — live name is **`ExportRiskBlocked`** (`error.rs`). Do not copy the wrong ident.

### 2.10 Research currency

| Claim | Source | Plan-time |
|---|---|---|
| Combined classify rewrite | `crates/pst-dedup-cli/src/unique_pst_cmd.rs` ~3416–3427, ~3536–3577 | live on HEAD `f49857e` |
| Cancel Ok zeros | `unique_eml_cmd.rs` ~449–471 | live |
| Recover helper | `also_eml_recovered_counts` | already used on unique-pst **Err** arm |
| 0078 `ok` / allow-partial / risk 65 | `export_outcome.rs` + `docs/unique-pst-export.md` ~605–626 | `ok == (fidelity == complete)` |
| Combined exit | `worse_cli_exit` | `130 > 1 > 65 > 64 > 0` — **keep** |
| MS-PST | N/A this track | CLI classify only |
| Schema / jobs | N/A (`matter-core` schema v39 unused) | — |
| Crate APIs | No new deps | re-verify at execute |

Re-verify line numbers at execute. `keep_set_v1` / `eml_pack_v1` / `unique_export_report_v1` **not** bumped.

---

## 3. In scope

### 3.1 Combined fidelity (not from exit)

Add `worse_export_fidelity(a, b) -> ExportFidelity` next to `worse_cli_exit`: **Failed > Partial > Complete**.

When also-eml **ran** (including also-eml cancel):

```
classified.fidelity = worse_export_fidelity(pst_classified.fidelity, eml_pack.fidelity)
```

When also-eml did **not** run: leave PST `classify_export` fidelity unchanged (`eml_fidelity = None`).

**Exit/reasons stay where they are.** Combined **exit** is already `worse_cli_exit` / also-eml-cancel 130 at ~3353–3415. Do **not** re-merge exit or reasons inside the fidelity helper.

**EML fidelity source:** add `fidelity: ExportFidelity` to `WriteEmlPackFromKeepSetResult`. Inner already classifies — copy `classified.fidelity`. Cancel `Err`→`Ok` conversion sets **Failed**.

Also-eml cancel: EML Failed → combined Failed; process `cancelled=true`; exit **130**. PST `artifact_state` stays the PST `--out` value from §2.3.

Risk-gate 65: PST Complete + EML Complete → combined **Complete** (not Failed). Exit stays **65**.

### 3.2 `ok` contract (all unique-pst, also-eml or not)

Replace `ok = combined_exit == Success && !process_cancelled` with:

```
ok = (classified.fidelity == Complete) && !process_cancelled
```

after combined fidelity is applied. This restores `docs/unique-pst-export.md`: `ok == (fidelity == complete)`.

`--allow-partial-fidelity` + attach/body soft-fail: `fidelity=partial`, `ok=false`, exit **0** (also-eml or not).

`UniquePstOutcome.ok`, summary `ok`, and `--json` stdout must agree.

`pub(crate)` helper (plan-time name: `finalize_unique_pst_classify`):

```
fn finalize_unique_pst_classify(
    pst: ExportOutcome,
    eml_fidelity: Option<ExportFidelity>,
) -> ExportOutcome
```

- If `eml_fidelity` is `Some(eml)`, set `pst.fidelity = worse_export_fidelity(pst.fidelity, eml)` and return `pst`.
- If `None`, return `pst` unchanged.
- Do **not** take EML `exit` / `reasons` / `cancelled`. Do **not** call `worse_cli_exit`. Do **not** mutate `artifact_state`.
- `ok` is computed **at the call site** after this helper: `ok = (classified.fidelity == Complete) && !process_cancelled`.

Unit-test the helper with constructed `ExportFidelity` values. Do **not** depend on aspose attach-fails (clean fixtures stay `Complete` and would false-pass an allow-partial integration test).

### 3.3 Summary-rewrite cancel (Bugbot M)

On `summary.json` write failure (~3536):

- Pass **`process_cancelled`** (`cancelled \|\| also_eml_cancelled`) into `classify_export`, not PST-only `cancelled`.
- Pass **`process_cancelled`** into `summary_is_retryable`.
- If `process_cancelled`, 0078 cancel short-circuit: exit **130**, reasons `[CANCELLED]` only (do **not** add `REPORT_WRITE_FAILED` — cancel suppresses findings), `retryable=true`, `ok=false`.
- If not cancelled: current `report_ok=false` → Generic **1** is correct (1 > 64). Keep that.

Extract `classify_after_summary_write_failure(base, risk, risk_gate, fail_on_partial, process_cancelled) -> ExportOutcome` so the 130 path is unit-tested without forcing a real disk-full rewrite.

Process exit (`AlreadyEmitted.exit` / `outcome.exit`) must stay **130** when the rewrite fires after also-eml cancel.

### 3.4 Cancel recovery (Bugbot L)

In `write_eml_pack_from_keep_set` cancel `Err`→`Ok` arm:

- Call `also_eml_recovered_counts(out)` for `(eml_written, attach_parts_failed, embedded_messages_written, volumes)`.
- `eml_written` may still fall back to `count_eml_under` **inside** that helper when summary JSON is missing.
- **Attach/embedded recover from summary/manifest JSON only.** If neither JSON is present, those two stay **0** even when `.eml` files exist (helper cannot audit attach fails from files). Do not promise otherwise.
- `attach_parts_written` on this cancel-Ok struct may stay 0 — unique-pst `also_eml_*` keys do not copy it. Do not expand the summary schema.
- Set `fidelity: Failed`, `exit: Cancelled`, `cancelled: true`.

unique-pst **Ok** arm already copies pack fields — fixing the helper fixes also-eml counters. unique-pst **Err** arm already recovers; do not double-zero.

Standalone unique-eml uses the same helper — the fix applies there too (in scope).

### 3.5 Tests (CI; no client PST)

| Test | Assert |
|---|---|
| `worse_export_fidelity_order` | Failed > Partial > Complete; equal returns left |
| `finalize_allow_partial_also_eml_stays_partial` | Helper: PST Partial + `Some(Complete)` → **Partial**. Call-site `ok=false`. **Do not** assert exit in the helper (exit merge is live code, not this fn). |
| `finalize_allow_partial_without_also_eml` | Helper: PST Partial + `None` → Partial. `ok=false` at call site. |
| `finalize_risk_gate_complete_stays_complete` | Helper: PST Complete + `Some(Complete)` → **Complete** (this is the 65 case: live exit stays `ExportRiskBlocked`; fidelity must **not** become Failed). |
| `finalize_eml_partial_marks_combined_partial` | Helper: PST Complete + `Some(Partial)` → Partial |
| `finalize_also_eml_cancel_failed_fidelity` | Helper: PST Complete + `Some(Failed)` → Failed |
| `classify_after_summary_write_failure_preserves_also_eml_cancel` | `process_cancelled=true` + `report_ok` would-be false → 130, retryable, only CANCELLED |
| `classify_after_summary_write_failure_report_fail_not_cancel` | `process_cancelled=false` → Generic 1, not retryable |
| `cancel_ok_recovers_attach_and_embedded_from_summary` | **Pinned trigger:** seed `{out}/summary.json` as a **file** with `eml_written=7`, `attach_parts_failed=2`, `embedded_messages_written=3`; create `{out}/manifest.json` as a **directory** (same Err as `helper_hard_fail_writes_summary_json`); `cancel=true`. Err→Ok copies **7/2/3**, not zeros. Do **not** block `summary.json` as a directory. |
| Existing `helper_cancel_with_blocked_summary_returns_cancelled_ok` | Stays 130; counts may stay 0 (no usable summary) |
| Existing `cancel_during_pst_write_skips_also_eml` | Unchanged |
| Existing 0078 / 0107 also-eml tests | Stay green |

Do **not** import INC* into git. Prefer `export_outcome.rs` `mod tests` + `unique_eml_cmd.rs` `mod tests` + the recovery test in `unique_pst_also_eml.rs`. Helper tests **inject** `ExportFidelity::Partial` / `Failed`; they must not rely on aspose attach-fails.

### 3.6 Docs

Additive only (do not rewrite 0078 tables):

- `docs/unique-pst-export.md` JSON-fields paragraph (~625–631): with `--also-eml`, `fidelity` / `ok` / `exit_code` are the **combined** job; `ok == (fidelity == complete)` still holds; `artifact_state` remains the PST `--out` disposition; also-eml pack has its own `{dir}/summary.json`.
- Same paragraph (or the allow-partial row ~620): after this track, `--allow-partial-fidelity` unique-pst summaries with `fidelity=partial` carry `ok=false` **and** `error.code=partial_fidelity` (retryable stays `false`). Previously `ok=true` hid the error object. Matches unique-eml. Automation that treated “no `error` key” as complete must read `fidelity` / `ok`.
- CHANGELOG Unreleased: also-eml classify/cancel honesty.
- Close `D-0109-also-eml-classify`.

---

## 4. Out of scope (do NOT do here)

- Re-scan / `run_unique_eml` / second keep-set.
- 0108 poly `effective_degraded_winner_rate` / keep-set CRC restrip (`D-0108-keepset-crc-retaint`).
- Changing 0078 exit integers or `worse_cli_exit` precedence.
- Adding `also_eml_fidelity` (or any new summary key).
- Schema id bumps.
- GUI wizard `--also-eml` checkbox.
- Matter / Relativity child-document extract (`D-0067-embedded-depth` — **do not close**).
- unique-eml `Bcc:` policy / `--include-bcc-recipients` default.
- HNBITMAPHDR. Frontend (0110+).
- In-tool ScanPST / CRC repair. Mutating source PSTs. Committing `output/` or INC* JSON.
- Recomputing PST `artifact_state` from combined Failed after also-eml-only cancel.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0107 Completed (`write_eml_pack_from_keep_set`, `also_eml_*` keys, `worse_cli_exit`, skip also-eml on PST cancel). 0078 `classify_export` / `summary_is_retryable`.
- *Verified to date:* three Bugbot sites live on `f49857e`; unique-pst Err arm already recovers counts; unique-eml `ok` already fidelity-based.
- Re-verify line numbers and `WriteEmlPackFromKeepSetResult` fields at execute.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Mapping fidelity from exit again | Helper + tests in §3.5; **delete** `match combined_exit` for fidelity |
| Double-merge of exit/reasons | Helper takes **fidelity only**; live ~3353–3415 stays the single merge |
| Risk 65 marked Failed | Explicit Complete+65 test |
| Allow-partial `ok: true` | `ok` from fidelity; unit test without aspose |
| Cancel rewrite drops 130 | `process_cancelled` into classify; helper test |
| Double-zero after Err arm | Only the cancel **Ok** conversion fills from recovered counts |
| PST `artifact_state` flipped on also-eml cancel | Do not recompute artifact_state after combined fidelity |
| Schema / oracle drift | No new keys; oracle still strips only `also_eml_out` |
| `unwrap` / `expect` | Forbidden in production |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Combined classify:** unique-pst `fidelity` is worse of PST + EML classified fidelities when also-eml ran (not derived from combined exit). `--allow-partial-fidelity` stays `fidelity=partial` / `ok=false` / exit **0** with or without `--also-eml`. Risk-gate **65** stays `fidelity=complete` (not Failed). Combined exit precedence unchanged (`130 > 1 > 65 > 64 > 0`). `ok == (fidelity == complete) && !cancelled`. PST `artifact_state` is `--out` only (§2.3). Pack result exposes `fidelity`. No production `unwrap`/`expect`. Source PSTs read-only.
- [ ] **DoD-2 — Cancel rewrite:** also-eml cancel + failed unique-pst `summary.json` rewrite → process exit **130**, `retryable` cancel-class, reasons `[CANCELLED]`. PST-only report fail without cancel still **1**.
- [ ] **DoD-3 — Cancel counts:** cancel `Err`→`Ok` recovers `also_eml_attach_parts_failed` / `also_eml_embedded_messages_written` / `eml_written` via `also_eml_recovered_counts`. Attach/embedded come from summary/manifest **JSON**; they may still be 0 when only `.eml` files exist. `eml_written` may count files when JSON is absent.
- [ ] **DoD-4 — Tests:** §3.5 names (or execute-time equivalents) green. Existing 0078 / 0107 also-eml tests stay green.
- [ ] **DoD-5 — Docs:** `docs/unique-pst-export.md` combined-job sentence; CHANGELOG Unreleased; `D-0109-also-eml-classify` **closed**.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; ledger commit (`BUGFIX` on `crates/pst-dedup-cli` at implement). No HITL required.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
$env:CARGO_TARGET_DIR = 'C:\dev\Dedupe\target'
cargo test -p pst-dedup-cli --lib export_outcome
cargo test -p pst-dedup-cli --lib unique_eml_cmd
cargo test -p pst-dedup-cli --lib unique_pst_cmd
cargo test -p pst-dedup-cli --test unique_pst_also_eml
cargo test -p pst-dedup-cli --test export_exit_0078
cargo fmt --all --check
cargo clippy -p pst-dedup-cli --all-targets -- -D warnings
# before implement-track publish:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Filter names re-verify at execute. No operator INC* command. Do not commit client PSTs or `output/`.

---

## 9. Deferred roll (mandatory)

Entire `docs/deferred.md` scanned 2026-08-29. Related open rows:

| Row | Disposition |
|---|---|
| **D-0109-also-eml-classify** | **Absorb / close.** Combined also-eml fidelity/ok/cancel rewrite + cancel counts. |
| **D-0108-poly-degraded-winner-risk** | **Decline.** Closed in 0108. |
| **D-0108-keepset-crc-retaint** | **Decline.** Keep-set CRC restrip residual after 0108. |
| **D-0071-also-eml** | **Decline.** Closed in 0107 (wiring). This track is classify honesty on that wiring. |
| **D-0067-embedded-depth** | **Decline.** Matter children residual. **Do not close.** |
| **D-0067-long-path** | **Decline.** |
| **D-0067-cloud-attaches** | **Decline.** |
| **D-0072-operator-gui-smoke** / wizard also-eml checkbox | **Decline.** |
| **D-0100-hn-bitmap-hdr** | **Decline.** Fail-closed until a corpus hits it. |
| **D-0094-inc-resmoke** | **Decline.** Closed HITL 2026-08-29. |
| **D-0062-codesign** | **Decline.** Release ops. |
| Other `docs/deferred.md` rows | **Decline** — not also-eml classify/cancel. |

Med/high never parked here. No BCC-default track. Frontend **0110+**. Next free ID **0117**. Fold-in (2026-08-29) did not change these dispositions.
