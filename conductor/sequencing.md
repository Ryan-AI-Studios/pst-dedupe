# Execution Sequencing — Dedupe Desk (0015–0072)

> **Plan-of-record:** `C:\dev\Dedupe-plan.md`  
> **Registry:** [`conductor.md`](conductor.md)  
> **Roadmap (high-level placeholders):** [`ROADMAP.md`](ROADMAP.md)  
> Track numbers are **stable IDs**, not strict execution order. This file is the order + concurrency view.  
> **Evidence:** real case PSTs stay outside the repo — CI uses `fixtures/` only.

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
| 4 | P0 Foundation | **0019** ProcessJobRunner | ✅ Completed | After 0015; unblocks **0020** / **0021** |
| 5 | P0 Foundation | **0018** PstExtractorAdapter | ✅ Completed | After **0016 + 0017**; unblocks **0021** (with 0019) |
| 6 | P0 Foundation | **0020** DeskShellUx | ✅ Completed | After **0019** (ideally after 0018 for real process demos) |
| 7 | P0 Reduce | **0021** MatterDedupeJob | ✅ Completed | MID + logical_hash job; family flags; schema v3 |
| 8 | P0 Reduce | **0025** PromoteToReview | ✅ Completed | After 0021–0024; `auto` = cull_included ∥ unique_only; family expand; unblocks **0026** |
| 9 | P0 Review | **0026** ReviewListViewer | ✅ Completed | After 0025 (`in_review` corpus); unblocks **0027** |
| 10 | P0 Review | **0027** CodingAndBatch | ✅ Completed | After 0026; catalog + batch apply + audit; closes MVP tag gate |
| — | **MVP GATE** | Desk opens Purview PST → dedupe → tag items → audit | — | Exit criteria in Dedupe-plan §7 P0; real PSTs operator-local only |
| 11 | P1 | **0022** EmailThreading | ✅ Completed | After 0018; parallel with 0023 historically; does not block 0025 |
| 12 | P1 | **0023** NearDuplicateDetection | ✅ Completed | MinHash shingles + LSH; schema v5 |
| 13 | P1 | **0024** CullAndReduce | ✅ Completed | After 0021; optional 0022/0023 rules; feeds 0025; schema v6 job `cull` |
| 14 | P1 | **0028** FiltersSavedSearch | ✅ Completed | Metadata FilterSpec + saved searches; schema v9; unblocks 0029 compose |
| 14b | P1 | **0029** KeywordFtsSearch | ✅ Completed | Tantivy 0.26.x; fts_index job; keyword ∩ filters |
| 14c | P1 | **0030** NotesHighlights | ✅ Completed | Schema v11 notes + stand-off highlights; whitespace re-resolve; desk panel |
| 14d | P1 | **0031** PrivilegeWorkflow | ✅ Completed | Schema v12 claims + withhold + privilege log CSV; after 0027/0030 |
| 14e | P1 | **0032** RedactionV1 | ✅ Completed | Schema v13 text redactions + true redacted CAS; after 0026/0030/0031 |
| 15 | P1 | **0033** OfficeExtractors | ✅ Completed | extract-office DOCX/XLSX/PPTX + job; schema v14; Codex luna PASS WITH DEFERRED P3 |
| 15b | P1 | **0034** PdfExtractPreview | ✅ Completed | extract-pdf text + pdf_needs_ocr; no pure-Rust preview; schema v15; Codex luna PASS WITH DEFERRED P3 |
| 15c | P1 | **0035** CalendarItems | ✅ Completed | extract-calendar + extract-pst calendar; schema v16; Codex luna PASS WITH DEFERRED P3 |
| 15d | P2 | **0036** OcrPlugin | ✅ Completed | Opt-in Tesseract CLI + mock; schema v17; job ocr; Codex luna PASS WITH DEFERRED P3 |
| 15e | P1 | **0037** FileCategoryTaxonomy | ✅ Completed | `taxonomy_v1` + file-category crate + job `classify`; schema v18; Codex luna PASS; unblocks **0038** type rollups |
| 15f | P1 | **0038** CaseOverviewDashboard | ✅ Completed | `CaseOverview` SQL rollups + desk Overview; schema v19 indexes; concurrent open_for_read fan-out; Codex luna PASS |
| 15g | P1 | **0039** ProgressReporting | ✅ Completed | Exportable matter_report_v1 CSV pack from CaseOverview + jobs; audit; PDF optional; after **0038** |
| 15h | P1 | **0040** ProductionExport | ✅ Completed | Produce volume natives+text+Concordance DAT; withhold/redacted gates; schema v20; job `produce`; Codex luna PASS |
| 15i | P1 | **0041** ProductionQcRules | ✅ Completed | Pre-produce QC pack + fingerprint produce gate; schema v21; job `qc`; Codex/internal PASS WITH DEFERRED P3 |
| 15j | P2 | **0042** GapAnalysis | ✅ Completed | Expected custodians + opposing DAT set-diff; schema v22; independent PASS WITH DEFERRED P3 |
| 16 | P1 | **0043** ProcessingProfiles | ✅ Completed | Named stage presets + sequential profile_run; schema v23 |
| 17 | P2 | **0044** WorkflowEngine | ✅ Completed | Declarative multi-node workflow_run; schema v24; parent_job_id |
| 18 | P1 | **0045** CliAutomationParity | ✅ Completed | Headless matter CLI on pst-dedup; shared register_default_handlers; after 0044 |
| 19 | P2 | **0046** EntityPiiPacks | ✅ Completed | Offline regex entity/PII packs + entity_scan; schema v25 |
| 20 | P2 | **0047** PeopleCommsGraph | ✅ Completed | Participants + directed edges + timeline; schema v26 |
| 21 | P2 | **0048** ClusteringConceptMining | ✅ Completed | Offline tfidf_kmeans_v1 + labels; schema v27 |
| 22 | P2 | **0049** SentimentNlpPlugin | ✅ Completed | Offline vader_lexicon_v1; schema v28 |
| 23 | P2 | **0050** SemanticSearchPlugin | ✅ Completed | Local embeddings semantic search; schema v29 |
| 24 | P2 | **0051** AiProviderTrait | ✅ Completed | AI provider trait + first-pass suggestions; schema v30 |
| 25 | P2 | **0052** AiReviewCitations | ✅ Completed | AI citations + human promote; schema v31 |
| 26 | P3 | **0053** TranscriptionPlugin | ✅ Completed | Local STT → transcript CAS + FTS; schema v32 |
| 27 | P2 | **0054** MultilingualPacks | ✅ Completed | CJK FTS n-gram + pack fingerprint; schema v33 |
| 28 | P2 | **0055** TeamsChatAdapters | ✅ Completed | Teams HTML+PST→items + conversation_id; schema v34 |
| 29 | P2 | **0056** ConversationReviewUi | ✅ Completed | Conversation list + paged message stream |
| 30 | P2 | **0057** SecurityHardener | ✅ Completed | Matter encryption at rest (DB+CAS+FTS) + unlock UX; schema v35 |
| 31 | P3 | **0058** MultiUserMatterService | ✅ Completed | Opt-in matter service + concurrent review; schema v36; unlocks **0059**/**0061** |
| 32 | P3 | **0059** MultiTenantSso | ✅ Completed | Platform tenant registry + OIDC SSO; schema v37 |
| 33 | P3 | **0060** MultiJurisdictionProduction | ✅ Completed | Production profiles + QC packs; schema v38 |
| 34 | P3 | **0061** CloudBlobJobBackends | 🔄 In Progress | Opt-in BlobStore + JobBackend; schema v39; offline default; Series I close |
| 35 | P0 cons. | **0062** ReleaseHardeningRc | ✅ Completed | RC freeze 0.2.0-rc.1 |
| 36 | P0 cons. | **0063** SecurityRedTeamFixes | 🔄 Completed | Series I red team + P0/P1 fixes on RC freeze |
| 37 | P1 cons. | **0064** DeskPlatformConnectUx | 📦 Proposed | Desk Connect + produce profile UX (native egui) |
| 38 | P0 K | **0065** ScanIntegrityReport | ⬜ Ready | (A) Multi-PST integrity: skip reasons, recoverable vs skipped |
| 39 | P0 K | **0066** DedupKeepSetExport | ⬜ Ready | (B) Keep-set v1 + decision log + materialize + EDRM MIH |
| 40 | P0 K | **0067** UniqueEmlPackCli | ✅ Completed | (C) Unique EML pack from keep_set_v1 + MIME multipart |
| 41 | P0 K | **0068** ProductionPstWriterV1 | ✅ Completed | (D) Writer v1: IPM_SUBTREE + XBLOCK full body |
| 42 | P0 K | **0069** PstWriterFidelity | ✅ Completed | (E) Attachments + folder path preserve under IPM |
| 43 | P1 K | **0070** PstWriterStreamingScale | ✅ Completed | (F) Multi-GB streaming write + stress |
| 44 | P0 K | **0071** CliUniquePstAndReport | ✅ Completed | (G) unique-pst CLI + report pack + verify |
| 45 | P2 opt. | **0072** DeskUniquePstWizard | ✅ Completed | (H) Optional GUI wizard over run_unique_pst |

## Series K spine (clean unique export)

```
0065 integrity (A)
  └─► 0066 keep-set (B)
        ├─► 0067 unique EML (C) ─────────────┐
        └─► 0068 writer v1 (D)               │
              └─► 0069 fidelity (E)          ├─► 0071 CLI + report (G) ─► 0072 Desk (H opt)
                    └─► 0070 streaming (F) ──┘
```

**Effort:** A–C + report ≈ weeks; D–E months; F + full fidelity multi-GB = major program.


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
