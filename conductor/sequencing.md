# Execution Sequencing — Dedupe Desk (0015–0061)

> **Plan-of-record:** `C:\dev\Dedupe-plan.md`  
> **Registry:** [`conductor.md`](conductor.md)  
> Track numbers are **stable IDs**, not strict execution order. This file is the order + concurrency view.

## Status legend

✅ Done · 🔄 In progress · ⬜ Ready (can start) · 📦 Proposed · ⛔ Blocked

## Spine (single-thread view)

```
0015 MatterStore
  ├─► 0016 PurviewIngest
  ├─► 0017 NormalizedItem ──► 0018 PstExtractorAdapter
  └─► 0019 ProcessJobRunner ──► 0020 DeskShellUx
         │
         ▼
      0021 MatterDedupeJob
         │
      0022 Threading (//)   0023 NearDup (//)
         │
      0024 CullAndReduce
         │
      0025 PromoteToReview
         │
      0026 ReviewListViewer ──► 0027 CodingAndBatch
         │
         ├─► 0028 Filters · 0029 FTS · 0030 Notes · 0031 Privilege · 0032 Redaction
         ├─► Series D file types / OCR
         ├─► Series E production / dashboards
         ├─► Series F workflows
         ├─► Series G intelligence / optional AI
         ├─► Series H Teams
         └─► Series I multi-user / SaaS
```

## Order table

| # | Phase | Track | Status | Concurrent? |
|---|---|---|---|---|
| 1 | P0 Foundation | **0015** MatterStore | ✅ Completed | Unblocks 0016 / 0017 / 0019 |
| 2 | P0 Foundation | **0017** NormalizedItem | ✅ Completed | After 0015; unblocks **0018** with 0016 |
| 3 | P0 Foundation | **0016** PurviewIngest | ✅ Completed | After 0015; parallel with 0017/0019 |
| 4 | P0 Foundation | **0019** ProcessJobRunner | ⬜ Ready | After 0015; parallel with 0016/0017 |
| 5 | P0 Foundation | **0018** PstExtractorAdapter | ⬜ Ready | After **0016 + 0017** |
| 6 | P0 Foundation | **0020** DeskShellUx | ⬜ Ready | After **0019** (ideally after 0018 for real process demos) |
| 7 | P0 Reduce | **0021** MatterDedupeJob | ⬜ Ready | After 0018 + 0019 |
| 8 | P0 Reduce | **0025** PromoteToReview | ⬜ Ready | Soft-dep 0024; can start with exact-dup-only promote after 0021 |
| 9 | P0 Review | **0026** ReviewListViewer | ⬜ Ready | After 0025 |
| 10 | P0 Review | **0027** CodingAndBatch | ⬜ Ready | After 0026 |
| — | **MVP GATE** | Desk opens Purview PST → dedupe → tag items → audit | — | Exit criteria in Dedupe-plan §7 P0 |
| 11 | P1 | **0022** EmailThreading | 📦 | After 0018; parallel 0023 |
| 12 | P1 | **0023** NearDuplicateDetection | 📦 | After 0017; parallel 0022 |
| 13 | P1 | **0024** CullAndReduce | 📦 | After 0021 (+ preferably 0022/0023) |
| 14 | P1 | **0028–0032** Review polish | 📦 | After 0026/0027 |
| 15 | P1 | **0033–0037** File types / OCR | 📦 | After 0017/0018 |
| 16 | P1 | **0038–0040** Dashboard + production starter | 📦 | After 0025+ |
| 17 | P1 | **0043–0045** Automation | 📦 | After reduce/process stable |
| 18 | P2 | **0041–0042**, **0046–0054**, **0055–0056** | 📦 | Intelligence / Teams |
| 19 | P3 | **0057–0061** | 📦 | Multi-user / SaaS |

## Concurrency rules

- **One primary implementer on matter schema (0015)** until merged — avoid migration fights.
- After 0015: **0016 / 0017 / 0019** can run in parallel (different crates/modules).
- **0018** needs both ingest + item model.
- UI (0020/0026) can mock matter APIs briefly, but DoD requires real store integration.
- Series G AI tracks must not make AI required for Series A–C DoDs.

## Desktop invariant (every track)

- No user-started Postgres/Redis/Docker for Desk edition.
- Background work is app-owned (threads/child processes with clean lifecycle).
- Optional plugins (OCR/AI/transcription) may spawn helpers **only when enabled**.

## Notes

- Legacy `track001`–`track011` folders remain historical; do not renumber them.
- If 0024 is delayed, 0025 may promote “all processed non-error items” or “unique-only” as an interim policy — document in 0025 review if so.
