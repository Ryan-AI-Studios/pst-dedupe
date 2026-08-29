# Track review: 0109-AlsoEmlClassifyHonesty

**Harness:** OpenCode (`opencode`)
**Track:** `conductor/0109-AlsoEmlClassifyHonesty`
**Date:** 2026-08-29
**Mode:** review only — no implement, no fold.

## Summary

Every load-bearing pin in spec §2.1 verified line-exact against live HEAD `f49857e`; the three
PR #104 Cursor Bugbot findings are **all still live on main** (no silent fix — Phase 0's
"confirm no silent main fix" is already satisfied). The §2.2 honesty table is arithmetically
correct against `classify_export`'s actual mapping, and the proposed fix
(`worse_export_fidelity` + `ok == (fidelity == complete) && !cancelled` + `process_cancelled`
into the summary-rewrite classify + recovered counts on cancel-Ok) lands on the right
subroutines with compiler-forced wire-ups in all the right places.

| Pin | Live @ `f49857e` | Verdict |
|---|---|---|
| PST `classify_export` call | `unique_pst_cmd.rs:3340-3346` | ✅ exact |
| Combined exit merge (keep) | `:3353-3383` — `worse_cli_exit` `:3366-3368`, also-eml-cancel → 130 `:3360-3365`, reason merge `:3362-3381`, `classified.exit = combined_exit` `:3383` | ✅ |
| **Bug 1** — fidelity from exit + `ok` from exit | `:3416-3424` (`match combined_exit`), `:3427` `ok = combined_exit == Success && !process_cancelled` | ✅ exact; `:3427` is unconditional → fires **without** `--also-eml` too, exactly as §1/§2.2 claim |
| **Bug 2** — rewrite re-classify with PST-only cancel | `:3545-3580` — `classify_export(forced, …, cancelled)` `:3550-3556` (PST-only flag), `summary_is_retryable(…, cancelled, …)` `:3572-3577`; best-effort rewrite `:3579` | ✅ (spec "~3536–3577"; write-fail captured `:3540-3541`) |
| **Bug 3** — cancel Err→Ok zeros | `unique_eml_cmd.rs:461-471` — `attach_parts_failed: 0`, `embedded_messages_written: 0` `:465-466`, `eml_written: count_eml_under(out)` `:463`, hard-fail-summary guard `:437`, fallback JSON `:454-460` | ✅ exact |
| `also_eml_recovered_counts` | `:222-261` — summary path (`eml_written` with `count_eml_under` fallback `:228-230`, attach `:231`, embedded `:232`, volumes `:233-242`), manifest fallback `:249-255` | ✅ |
| Recovery already used on unique-pst **Err** arms | `unique_pst_cmd.rs:3170-3174` (cancel), `:3202-3206` (hard-fail) — cancel **Ok** conversion is the only zero path | ✅ |
| `WriteEmlPackFromKeepSetResult` has no `fidelity` | `unique_eml_cmd.rs:181-191` (has `exit`/`exit_reasons`/`cancelled`/counts) → adding the field is compiler-forced at the inner return sites | ✅ |
| **Bug 3 fix site reaches unique-pst Ok arm** | Ok arm copies `pack.eml_written` / `attach_parts_failed` / `embedded_messages_written` `:3116-3123` — no separate unique-pst edit needed | ✅ |
| unique-eml standalone `ok` already fidelity-based | `unique_eml_cmd.rs:740` `ok = fidelity == Complete && !cancelled` | ✅ exact |
| 0078 classify cancel short-circuit | `export_outcome.rs:163-170` — `Failed` / `Cancelled` / reasons `[CANCELLED]` only → §3.3's "do not add REPORT_WRITE_FAILED" is **automatic** once `process_cancelled` is passed | ✅ |
| 0078 allow-partial → exit 0, fidelity Partial | `:227-232`; risk-gate 65 keeps fidelity **Complete** `:225-226` | ✅ |
| Precedence rank, not raw `u8` | `:246-256` (Cancelled=5, Generic=4, Risk=3, Partial=2, Success=0) — `130 > 1 > 65 > 64 > 0` | ✅ |
| `summary_is_retryable` cancel-retry | `:301-304` → `process_cancelled` yields `retryable=true` | ✅ |
| Rewrite-fail non-cancel → Generic 1 correct | retryable via `reason::REPORT_WRITE_FAILED` → permanent `:346-357` | ✅ |
| Process exit from `classified.exit` | `:3646-3656` — `AlreadyEmitted { exit: classified.exit }`; once the rewrite classifies with `process_cancelled`, exit 130 propagates to the shell with **no extra plumbing** | ✅ |
| `artifact_state` computed pre-merge, not recomputed | `:3349-3350` (before combined merge), rewrite block `:3557-3579` never touches it → §2.3 lock already structural | ✅ |
| `export_exit_0078` ok-assert is aspose-gated | `tests/export_exit_0078.rs:158-161` — `if failed2 > 0 && fidelity partial` → clean fixtures skip `ok:false`, why CI missed 0107 | ✅ |
| Existing also-eml tests | `tests/unique_pst_also_eml.rs:800` `cancel_during_pst_write_skips_also_eml`, `:848` `helper_cancel_with_blocked_summary_returns_cancelled_ok` (blocked via **directory** `:903`, counts unasserted) | ✅ |
| Doc contract | `docs/unique-pst-export.md:626` `ok == (fidelity == complete)`; `:620` allow-partial row exit 0 | ✅ |
| Deferred row | `docs/deferred.md:888` D-0109-also-eml-classify **open / 0109** | ✅ |
| §8 filter commands | in-module tests exist: `export_outcome` (test `:529`), `unique_eml_cmd` (mod tests incl. `:1530`), `unique_pst_cmd` (`:4303`); integration tests `unique_pst_also_eml`, `export_exit_0078` exist | ✅ all commands valid |
| GUI blast radius | `unique_wizard.rs:367` `also_eml: None` — GUI never sets also-eml; frontend untouched | ✅ |

Online research: **N/A with justification** — pure CLI classify/policy JSON work; no MS-PST /
NDB / property-tag surface, no new crates, no external API. The serde contract is additive
(`WriteEmlPackFromKeepSetResult` is internal; summary keys unchanged), so nothing to
re-verify online.

Architecture is sound: fidelity is pulled from classified outcomes (never the integer), the
cancel short-circuit already suppresses finding-style reasons so §3.3's reason vocabulary is
free, and the 130 process exit self-propagates through `AlreadyEmitted` once the rewrite
classify sees `process_cancelled`. The residual gaps are AC-mechanics pins, not design flaws.

## Findings (B/M/m/O)

No B, no M. Two m, four O.

### m1 — Pin the `finalize_unique_pst_classify` boundary: fidelity only, or exit+reasons too?

§3.2/plan Phase 1 say the helper takes "optional EML `(fidelity, exit, cancelled, reasons)`"
and merges "using existing `worse_cli_exit` + new fidelity worse-of" — but §3.1 also says
combined exit is "already worse_cli_exit / also-eml-cancel 130 (do not change)". Live code
merges exit **and** reasons *before* the Bug 1 site (`:3353-3415`), including the
eml-worse-first reason-order branch (`:3369-3381`) and the `String → &'static` reason
re-mapping (`:3385-3414`). Two readings:

1. Helper handles **fidelity only** (exit/reason merge stays at :3353-3415) — then why does it
   take `exit`/`reasons`?
2. Helper absorbs the whole merge — then the :3385-3414 static re-map must move inside it,
   and the §3.5 unit tests should pin the merged reason **order** (`[CANCELLED, …]`,
   worst-first, eml reasons ahead of PST when the EML exit is worse).

Reading 1 is the minimal diff and matches "do not change" in §3.1. Fold one sentence that
says which; otherwise the implementer risks a **double-merge** (caller merges exits, helper
merges again — harmless for `worse_cli_exit` but can duplicate/reorder `exit_reason` in the
summary) or an unused-function-field clippy hit under `-D warnings`. Recommend: helper =
fidelity worse-of + `ok` contract; exit/reason merge left in place; helper takes only
`(pst_outcome, eml_fidelity: Option<ExportFidelity>)`. If the fuller signature is kept, add a
reason-order assertion to the §3.5 finalize tests.

### m2 — `cancel_ok_recovers_attach_and_embedded_from_summary` needs a pinned deterministic `Err` trigger

Spec §3.5 seeds `{out}/summary.json` with counts 7/2/3 and expects the cancel Err→Ok arm to
recover them. Verified the path works **if** `write_eml_pack_from_keep_set_inner` errs while
the seeded summary stays usable: `:437` skips the hard-fail summary (usable-summary guard,
proven by `hard_fail_summary_does_not_clobber_usable_summary` `:1530-1608`), `:451-460` reads
the seeded JSON back, and a fixed arm would return 7/2/3. But the **only known Err trigger**
in tests is blocking `summary.json` itself as a directory (`:903`) — which makes the seeded
summary unreadable and recovery legitimately zeros. The plan never says what makes inner fail
*and* leaves a usable summary behind. Pin a deterministic trigger in plan Phase 3, e.g. seed
`summary.json` as a valid file **and** make `manifest.json` a directory so the manifest
write errs. Unpinned, this test either can't be written as specified or silently degrades to
another blocked-dir test that proves nothing about recovery (the exact weakness §2.1 row 12
already documents for the existing helper test).

### o1 — DoD-3 letter vs helper reality for attach/embedded zeros

`also_eml_recovered_counts` recovers attach/embedded **only** from summary/manifest JSON
(`:231-232`, `:254-255`); with no JSON on disk the fallback is `0` even when `.eml` files
exist (only `eml_written` is file-counted, `:245`). §3.4's "zeros only when nothing is on
disk" is literally about `eml_written`; attach/embedded can still be zero with EMLs present.
Acceptable (counts unauditable without manifest), but DoD-3's wording could be read as
promising more than the helper delivers — one clarifying word ("counters recoverable from
JSON") or accept as-is.

### o2 — Error-field behavior delta on allow-partial (no also-eml)

Today an allow-partial PST run has `ok=true` → `summary_error = None` (`:3429`). After the
§3.2 fix, `ok=false` → the error chain at `:3430-3449` falls through to
`("partial_fidelity", "unique-pst partial fidelity")` unless an earlier error code wins
(writer/scan/etc. take precedence). That's *more* honest and matches unique-eml's shape, but
it means previously-error-free partial summaries now carry `error.code` — and
`summary_is_retryable` then reads `"partial_fidelity"` → permanent (correct, already in the
permanent list `:327-328`). Worth one sentence in the §3.6 docs edit so automation tracking
`error` isn't surprised; not a code change.

### o3 — §2.2 row 3 copy nit

"(accidentally ok if exit is 64)" — live `ok` is `exit == 0`, so a 64 partial is `ok=false`
today; the parenthetical overstates. Harmless diagnosis-table wording; fix only if the file
is touched anyway.

### o4 — Line drift is small and self-correcting

Bug 2 block: spec says ~3536, live capture at `:3540-3545`, classify `:3550`, rewrite
`:3579`. Phase 0's re-verify covers it; no action.

## What looks solid

- **All three Bugbot findings reproduce from the pins.** Bug 1's "fires without --also-eml"
  is visible in the unconditional `:3427`; Bug 2's 130→1 flip is exactly `:3550-3577` taking
  PST `cancelled`; Bug 3's zeros are exactly `:465-466`.
- **The fix is structurally cheap.** `artifact_state` is already computed pre-merge
  (`:3349-3350`) and never recomputed in the rewrite block — §2.3's don't-recompute lock
  needs zero code. Cancel 130 propagates through `AlreadyEmitted.exit` (`:3649-3656`) for
  free once classify sees `process_cancelled`. unique-pst's Err arms already recover counts,
  so only the Ok-cancel conversion (`:461-471`) needs the helper call — and the Ok arm copy
  (`:3116-3123`) then fixes unique-pst automatically.
- **0078 cancel semantics are reused, not reinvented.** classify's cancel short-circuit
  (`:163-170`) already yields 130 / `[CANCELLED]`-only / `cancelled: true`, and
  `summary_is_retryable` short-circuits cancel → true (`:301-304`) — §3.3 falls out of
  passing one flag.
- **Compiler-forced wire-ups.** Adding `fidelity` to the result struct and new helper params
  makes every construction site (`:461-471`, inner returns) a compile error until updated —
  no silent partial wiring.
- **Test economics are right.** §3.5 avoids aspose dependence; the §8 filters map onto real
  in-module `#[cfg(test)]` blocks (verified `:529` export_outcome, `:1530` unique_eml_cmd,
  `:4303` unique_pst_cmd) and both integration files exist. The recovery test's seeded
  summary is proven readable by existing test `:1537` (same JSON shape).
- **`ok` contract restores cross-command consistency:** unique-eml `:740` vs unique-pst
  `:3427` divergence is exactly the Bug 1 delta; the fix converges them with no exit-integer
  churn (`worse_cli_exit` untouched, oracle/allowlist untouched, `unique_export_report_v1`
  un-bumped, no `also_eml_fidelity` key — pack summary already carries `fidelity` at `:782`).
- **0107 isolation locks respected:** `cancel_during_pst_write_skips_also_eml` (`:800-845`)
  pins skip-on-PST-cancel; §4 declines re-wiring; GUI `also_eml: None` unaffected.

## Deferred fold-in table

All §9 rows verified against `docs/deferred.md` live:

| Row | Live state | Spec disposition | Verdict |
|---|---|---|---|
| **D-0109-also-eml-classify** | :888 — open / 0109 (Ready, absorb on Implement); matches §2.8 | Absorb / close | ✅ |
| D-0108-poly-degraded-winner-risk | :886 — closed / 0108 | Decline | ✅ |
| D-0108-keepset-crc-retaint | :887 — residual / after 0108 | Decline | ✅ |
| D-0071-also-eml | closed / 0107 (wiring; this track is classify honesty) | Decline | ✅ |
| D-0067-embedded-depth | open residual — **do not close** | Decline | ✅ |
| D-0100-hn-bitmap-hdr | :889 residual — fail-closed until corpus | Decline | ✅ |
| D-0094-inc-resmoke | :885 — closed HITL 2026-08-29 | Decline | ✅ |
| D-0072-operator-gui-smoke / D-0062-codesign | residual / release ops | Decline | ✅ |

No med/high open row on the classify/cancel surface outside D-0109 — §9's closing claim holds.

## Cursor / last-PR comments the plan missed

PRs #108, #107, #106, #105 merged (gh verified); origin Bugbot is **#104** (3 findings, still
live — verified in code above). `gh pr view 104/106 --json comments` show 0 surviving comment
bodies on `main`-adjacent PRs; §2.8's table records #106 as 0108-poly (none here), #108/#107/
#105 as chore/docs (none). Disposition "absorb all three here" is correct; no new placeholder;
**next free ID 0117** stands.

## Research / tools notes

- ai-brains: used from `C:\dev\Dedupe` — `preflight --summary` (inited; 3883 pinned; discovery
  grants empty 0/3 warning only), `sync query` + `recall --semantic` on combined-exit/0109
  (decision `3a1fc687` recovered — matches this spec verbatim: worse-of fidelity, ok contract,
  130 rewrite, recovered counts, artifact_state lock, frontend 0110+, no BCC, next 0117;
  0107 decision `816ac302` confirms `worse_cli_exit` precedence lesson — spec §2.1 embeds it
  correctly). Semantic recall reachable this pass — §2.4's "semantic recall timed out" did not
  reproduce; no contradiction with the plan.
- ledgerful: used from `C:\dev\Dedupe` — `doctor --json` readyForPublish:true; `ledger status
  --compact` **0 pending / 0 unaudited drift**; `scan --impact` LOW (dirty tree = conductor
  registry status bumps + `agy-review.md` at root + `.claude` junction; no product crates in
  the diff); planning tx `3db3c68e` + doc tx `9669a94f` already committed for this pass
  (§2.4 accurate). 0108's ledger chain confirms the plan's entity/category choice
  (`pst-dedup-cli` / Bugfix, tx `f6530ff3` precedent).
- Online research: N/A confirmed — no MS-PST/NDB/RFC surface; classify + JSON contract is
  internal-only; no new dependencies (`§2.10` "No new deps" verified against the plan's
  helper list).
- Phase 4's `ROADMAP.md` target **exists** at HEAD (created since the 0108 planning pass;
  registry rows already bumped to "Ready" in the dirty tree) — the 0108-era "ROADMAP.md
  missing" finding is obsolete; nothing to fold.
- HITL: correctly waived — all three findings are CI-testable with in-repo fixtures; the
  optional INC* re-smoke stays out of DoD per §HITL note.

## Verdict: Ready after fixes

No B/M findings. Fold in before implement start:

1. **m1** — pin `finalize_unique_pst_classify` scope: fidelity worse-of + `ok` contract only
   (recommended), leaving the `:3353-3415` exit/reason merge untouched — or, if the helper
   takes the full EML tuple, add a merged reason-order assertion to the §3.5 finalize tests.
2. **m2** — pin the recovery test's deterministic Err trigger (seed `summary.json` as a
   **file**, block `manifest.json` as a directory) so DoD-3's test can't silently degrade to
   the blocked-dir shape that proves nothing.
3. **o1** (wording) — DoD-3: attach/embedded recover from summary/manifest JSON only; zeros
   still possible when only `.eml` files survive.
4. **o2** (docs) — §3.6 sentence: partial-fidelity summaries now carry
   `error.code="partial_fidelity"` and `ok=false` (retryable stays false) — parity with
   unique-eml.

`/foldin 0109` folds this file into spec/plan (fold review files only; do not implement here).