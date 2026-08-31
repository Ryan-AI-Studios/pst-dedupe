# 0122 — Process-fold residuals (PR #123 Bugbot)

> Placeholder minted 2026-08-31 while expanding **0117**. Do **not** steal into
> the first-pass queue. Expand with `/plan-track 122` before Implement.

- **Track ID:** 0122-ProcessFoldResiduals
- **Status:** Proposed — placeholder
- **Series:** O
>
> Two **valid** Cursor Bugbot findings on PR **#123** (`727c857`) that live in
> `crates/dedupe-chrome/ui/src/pages/process.rs`. Not the queue (**0117**).
> Cancelled-produce-as-success lives on **0119** (`produce.rs`), not here.

## 1. Objective

Keep **0116** Process honest under single-flight extract-all and live job
rows: a Busy click must not drop the remaining PST queue, and a running job
must not stay painted as an orphan because `orphan` was captured at mount.

## 2. In scope (sketch)

PR #123 Bugbot (live-verified 2026-08-31 on HEAD `3bde470` / product `727c857`):

1. **Medium — Extract-all Busy wipes active queue** —
   `extract_all` overwrites `extract_queue` then clears it on any
   `process_start` error, including `Busy`. A second click while the first
   extract is running drops remaining PSTs. Do not clear the queue on Busy;
   ignore a second Extract all while a queue is already draining.
2. **Medium — Live jobs shown as orphans** —
   Job rows compute `orphan` / `active` once from `progress` when the `For`
   child mounts. A live `running` job that appears before the first poll
   stays labeled orphan (Resume primary, no row Pause). Derive orphan/active
   from the **current** `progress` signal inside the row (reactive), not a
   one-shot bool.

## 3. Out of scope

Queue virtualization (**0117**), window async (**0118**), produce wizard
Finalize / cancelled-as-success (**0119**), raster UI (**0120**), image QC
(**0121**). Do not change `process-runner` Busy semantics. Do not re-open
0116 DoD. No schema bump. No BCC.

## 4. DoD (sketch)

- [ ] Second Extract all while Busy does not empty `extract_queue`; remaining
      PSTs still dispatch after the in-flight job.
- [ ] A `running` job whose snapshot `job_id` matches the row shows Pause, not
      the orphan Resume/Cancel pair, once progress has polled.
