# 0125 — Produce canvas (not a wizard)

> Placeholder minted 2026-08-31 from `C:\dev\deviations.md` vs mock
> `C:\dev\dedupe-frontend` `pages/produce.rs`. Expand with `/plan-track 125`
> before Implement. Do **not** steal **0119** Bugbot honesty.

- **Track ID:** 0125-ProduceCanvas
- **Status:** Proposed — placeholder
- **Series:** T (mockup chrome fidelity)
- **Depends on:** **0113 / 0115 Completed** · **0123** shell · schema **v41**
>
> Live produce is **two columns** with **tabbed** steps 1–5 and Finalize on
> every step. Mock is three panes: sets+protocol | all five steps on one
> canvas | Stage/export.

## 1. Objective

Show the whole production decision on one canvas so counsel can see
blockers, Bates projection, and export paths **before** Finalize. Split
Stage vs Finalize. Disable Finalize while pre-flight fails.

0113 honesty stays: privilege-in-set hard block, `fail_if_withheld`,
`require_qc_pass`, no fake categorical log.

## 2. In scope (sketch)

From `C:\dev\deviations.md` §4 (verified live `produce.rs` `step` Show):

1. **Left:** production set stack (empty state + **New**); protocol block
   even if values are “none on file”; audit footnote.
2. **Center:** steps 1–5 **visible** (not a wizard that hides the rest).
   Counts on Set; pad width / last Bates / page- vs doc-level (page-level
   only when the **0115** image profile is selected). Image chips honour
   `us_concordance_image_opt_v1`; hide unimplemented categorical log
   (**D-0031-03**).
3. **Right Stage (320px):** docs/pages/natives/slipsheets/marks/withheld;
   export path list; Stage & snapshot vs Finalize (Finalize disabled on
   blockers). Move the always-on left-column Finalize here.
4. Status-bar flag (0123): privileged-doc rule.

## 3. Out of scope

**0119** Finalize re-arm / empty `filter_ids` / QC leak / cancelled-as-success
— those stay Bugbot. **0120** overlay coords. **0121** OPT QC eligibility.
**0123** shell. **0124** queue. No BCC. No schema bump. Do not weaken
privilege-in-set.

## 4. DoD (sketch)

- [ ] All five steps visible without changing a tab to discover QC.
- [ ] Stage pane present; Finalize disabled while blockers remain.
- [ ] Protocol block renders “none on file” rather than omitting the pane.
- [ ] 0113 privilege-in-set / `require_qc_pass` tests still pass.
