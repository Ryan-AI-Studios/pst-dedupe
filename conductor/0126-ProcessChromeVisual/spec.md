# 0126 — Process chrome visual (jobs table + reconciliation)

> Placeholder minted 2026-08-31 from `C:\dev\deviations.md` vs mock
> `C:\dev\dedupe-frontend` `pages/process.rs`. Expand with `/plan-track 126`
> before Implement. Do **not** steal **0122** extract-all Busy / orphan rows.

- **Track ID:** 0126-ProcessChromeVisual
- **Status:** Proposed — placeholder
- **Series:** T (mockup chrome fidelity)
- **Depends on:** **0116 Completed** · **0123** shell · schema **v41**
>
> Live Process already has three panes, in-page tabs, `error_groups`, and
> reconciliation **chips**. The dump overstated emptiness; the miss is
> **presentation** (jobs table, locked profile, drop copy, minus-stack).

## 1. Objective

Present 0116’s honest counts in the mockup Process layout: per-source
status, a jobs **table** (not a stack of `kind · state`), grouped
exceptions with a detail panel, and a minus-stack reconciliation that
shows **0** instead of `—` once DeNIST/dupes jobs have run.

## 2. In scope (sketch)

From `C:\dev\deviations.md` §2 (re-verified live `process.rs`):

1. **Sources:** drop-zone **copy** (PST · OST · MBOX · hashed on arrival)
   even if pickers stay (Tauri drop may remain **D-0116-drop** if not
   wired). Strip `\\?\` from display names. Progress bar when a job is
   live. One **locked** profile checklist mapped from builtins; near-dup
   off unless chosen. Extract/run stay; they must not dominate the pane.
2. **Jobs table:** Source, Items, Dupes, NIST, Families, Except., Status;
   Pause + download report. Fill Dupes/DeNIST from live counters; do not
   invent NSRL. Keep Pause/Resume/Cancel.
3. **Exceptions:** heading includes quarantine count; select a group;
   detail actions (retry/exclude) only if host already can — else honest
   empty actions. Empty state when count is 0 is fine.
4. **Reconciliation:** minus-stack (Discovered − DeNIST − dupes −
   quarantined → review-ready). Unaccounted-for stays first-class (0116
   lock). **Open review-ready** stays; **Download reconciliation** absorbs
   **D-0116-report** if 0039 CSV can be called without a schema bump.
5. Status bar (0123): job n of m · % · profile · SHA-256 identity.

## 3. Out of scope

**0122** extract-all Busy wiping the queue; mount-time orphan paint.
**D-0116-workflow** picker (unless a one-line residual). **0123** shell
chrome (consume it). **0124** / **0125**. OST/MBOX ingest engines
(**D-0016-05**). No schema bump. No BCC. Do not fake NSRL counts.

## 4. DoD (sketch)

- [ ] Jobs render as a table with live columns; no `—` for DeNIST/dupes
      after those stages have run (0 is allowed).
- [ ] Source paths do not show a `\\?\` prefix; drop-zone copy is visible.
- [ ] Reconciliation is a minus-stack; unaccounted-for still 0 only when
      0116’s extract-success rule holds.
- [ ] D-0116-report closed if download ships; else remain with a reason.
- [ ] 0122 Bugbot still a separate track.
