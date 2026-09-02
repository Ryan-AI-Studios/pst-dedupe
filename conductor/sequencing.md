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
| 34 | P3 | **0061** CloudBlobJobBackends | ✅ Completed | Opt-in BlobStore + JobBackend; schema v39; offline default; Series I close |
| 35 | P0 cons. | **0062** ReleaseHardeningRc | ✅ Completed | RC freeze 0.2.0-rc.1 |
| 36 | P0 cons. | **0063** SecurityRedTeamFixes | ✅ Completed | Series I red team + P0/P1 fixes on RC freeze |
| 37 | P1 cons. | **0064** DeskPlatformConnectUx | ✅ Completed | Desk Connect + Solo produce profile UX (native egui); researched 2026-07-25 |
| 38 | P0 K | **0065** ScanIntegrityReport | ✅ Completed | (A) Multi-PST integrity: skip reasons, recoverable vs skipped |
| 39 | P0 K | **0066** DedupKeepSetExport | ✅ Completed | (B) Keep-set v1 + decision log + materialize + EDRM MIH |
| 40 | P0 K | **0067** UniqueEmlPackCli | ✅ Completed | (C) Unique EML pack from keep_set_v1 + MIME multipart |
| 41 | P0 K | **0068** ProductionPstWriterV1 | ✅ Completed | (D) Writer v1: IPM_SUBTREE + XBLOCK full body |
| 42 | P0 K | **0069** PstWriterFidelity | ✅ Completed | (E) Attachments + folder path preserve under IPM |
| 43 | P1 K | **0070** PstWriterStreamingScale | ✅ Completed | (F) Multi-GB streaming write + stress |
| 44 | P0 K | **0071** CliUniquePstAndReport | ✅ Completed | (G) unique-pst CLI + report pack + verify |
| 45 | P2 opt. | **0072** DeskUniquePstWizard | ✅ Completed | (H) Optional GUI wizard over run_unique_pst |
| 46 | P2 M | **0088** SovereignCloudHosts | ✅ Completed | Sovereign SP/OneDrive + office365.us SafeLinks (closes D-0085; residual D-0088-usgovcloud-microsoft-tld) |
| 47 | P1 M | **0089** UniqueEmlAttachLedger | ✅ Completed | unique-eml attach ledger via EmlAttachEvent (closes D-0073-eml) |
| 48 | P2 M | **0090** EmbeddedMsgContentHash | ✅ Completed | embedded-msg-hash/v1 subnode+rfc822 (D-0086-embedded-email-hash) |
| 49 | P2 M | **0091** DigestProbeUnify | ✅ Completed | Record-don’t-tee digest→probe skip (D-0086-digest-probe-unify) |
| 50 | P2 M | **0092** CloudNamedPropWrite | ✅ Completed | Allowlisted NPMAP write (D-0084-cloud-named-prop-write); residual D-0092 → **0096** |
| 51 | P1 N | **0093** WriterHeapRecipientRobustness | ✅ Completed | Strategy B + cumulative heap; closes D-0068-01; residuals D-0093-*; Codex luna r4 PASS |
| 52 | P1 N | **0094** EmbeddedMsgNestedExport | ✅ Completed | Method-5 nested export + PtypObject; D-0069 closed; D-0067 narrowed; Codex r5 PASS |
| 53 | P2 N | **0095** UniquePstFolderTreeNormalize | ✅ Completed | Alias strip + lazy Unique Mail + D-0070 pre-seed closed; QC key symmetry + DI claimable |
| 54 | P3 N | **0096** PermissionTypeExtract | ✅ Completed | Four-crate extract + PtypInteger32; closes D-0092-permission-type-extract |
| 55 | P3 N | **0097** BodyCloudTruncationHonesty | ✅ Completed | C+A hybrid + split window_capped; closes D-0097-body-cloud-truncate-honesty |
| 56 | P0 P | **0099** CrcPolyExportRiskHonesty | ✅ Completed | Effective (non-poly) CRC rate for unique-pst `export_risk`; D-0077-systematic-poly honesty half |
| 57 | P0 P | **0100** RecipientTcMultipage | ✅ Completed | Strategy A recipient TC; D-0093-recipient-tc-multipage |
| 58 | P1 P | **0101** EmbeddedDepthFlag | ✅ Completed | unique-pst `--max-embedded-depth`; D-0067 CLI half; PR #92 |
| 59 | P2 P | **0102** ExportOracleInputsAttest | ✅ Completed | oracle `export_risk.inputs` attest; D-0099-oracle-inputs-attest |
| 60 | P2 P | **0103** RecipientTcSlblockNidOrder | ✅ Completed | SLBLOCK NID order; D-0100-slblock-nid-order; PR #96 |
| 61 | P2 P | **0104** AttachmentTcMultipage | ✅ Completed | attach-table Strategy A; D-0093-attachment-tc-page |
| 62 | P3 Q | **0105** BodyCloudWindowEdgeNormalize | ✅ Completed | window-edge `normalize_candidate`; D-0097-window-edge-normalize |
| 63 | P1 Q | **0106** UniqueEmlNestedMime | ✅ Completed | unique-eml nested RFC 5322 from NestedCanonicalMessage; D-0067 unique-eml half |
| 64 | P1 R | **0107** UniquePstAlsoEml | ✅ Completed | unique-pst `--also-eml` same keep-set unique-eml pack; D-0071-also-eml; PR #104 / `339dfa0` |
| 65 | P1 S | **0108** PolyDegradedWinnerRisk | ✅ Completed | effective degraded_winner_rate excludes poly-only CrcSuspect on poly sources; D-0108 |
| 66 | P2 S | **0109** AlsoEmlClassifyHonesty | ✅ Completed | PR #104 Bugbot also-eml classify/cancel; D-0109; PR #109 / `dc7c29c` |
| 67 | P1 O | **0110** MatterChromeTauri | ✅ Completed | Tauri 2 + Leptos matter chrome; one `matter_overview`; PR **#111** `5a76f0b` |
| 68 | P1 O | **0111** ReviewQueueFirstPass | ✅ Completed | virtualized first-pass queue; PR **#113** / `3c4ca65` |
| 69 | P0 O | **0112** ReviewWindow | ✅ Completed | three-pane coding; Resp ⊥ Privilege; PR **#115** `81a3aad` |
| 69b | P3 O | **0117** QueueVirtualizationResiduals | ✅ Completed | PR #113 queue Bugbot; PR **#125** / `199975c` |
| 70 | P1 O | **0113** ProduceChecklist | ✅ Completed | produce checklist; DAT only; PR **#117** / `f192b2d` |
| 71 | P1 O | **0114** PdfRasterRedact | ✅ Completed | zpdf CPU raster + geometric burn; schema v40; PR **#119** / `5ed53bf` |
| 72 | P2 O | **0115** ImageOptFactory | ✅ Completed | TIFF G4 + OPT; page-level Bates; PR **#121** / `19d0c1f` |
| 73 | P2 O | **0116** ProcessFold | ✅ Completed | fold egui Process into Tauri; absorb D-0113-long-job; PR **#123** / `727c857` |
| 74 | P3 O | **0118** ReviewWindowAsyncResiduals | ✅ Completed | PR #115 window Bugbot; PR **#127** / `74fd797` |
| 75 | P3 O | **0119** ProduceChecklistResiduals | ✅ Completed | PR #117 produce Bugbot + PR #123 cancelled/idle; PR **#129** / `6a775b5` |
| 76 | P3 O | **0120** PdfRasterUiResiduals | ✅ Completed | PR #119 Image-tab/Burn-count Bugbot; PR **#131** / `e87f4c1` |
| 77 | P3 O | **0121** ImageOptQcResiduals | ✅ Completed | PR #121 image QC/eligibility Bugbot; PR **#135** / `600d6b3` |
| 78 | P3 O | **0122** ProcessFoldResiduals | ✅ Completed | PR #123 Process extract-all/orphan Bugbot; PR **#137** / `f1810fe` |
| 79 | P3 T | **0123** MatterShell | ✅ Completed | Shared TopBar/StatusBar; Home under bar; PR **#139** / `fce416e` |
| 80 | P3 T | **0124** ReviewQueueChrome | ✅ Completed | Ellipsis; 244px rail; Go-to + SQL-page range; PR **#141** / `ff8b0ea` |
| 81 | P3 T | **0125** ProduceCanvas | 📦 Proposed | Un-wizard produce; 0119 Bugbot stays |
| 82 | P3 T | **0126** ProcessChromeVisual | 📦 Proposed | Jobs table / minus-stack; 0122 Bugbot stays |

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

## Series L - Unique export hardening (0073-0081)

| Order | Track | Notes |
|---|---|---|
| 1 | **0073** Attach failure ledger | **Completed** — locus CSV + reason codes + histogram; unblocks 0074/0077/0081 |
| 2 | **0074** Deep attach preflight | **Completed** — budgeted L2, peer-cap, cache→stream_available; D-0074-* residual |
| 2b | **0077** CRC noise + risk | **Completed** — data-path CRC telemetry, CRC_SUSPECT (split-only Tier-2), dual-rate poly reclassify, `export_risk` on PreflightRecommendation, attach stream CRC→risk, Desk banner; Codex luna PASS WITH DEFERRED P3; feeds **0078** / **0081** |
| 2c | **0078** Exit codes | **Completed** — `classify_export`, exit 64/65/130, cancel quarantine, JSON contract; feeds **0080** QC / **0081** runbook (anti-recommendation: no blanket retry exit 5 / `AuditChainBroken`); Codex luna PASS WITH DEFERRED P3 |
| 3 | **0075** Winner policies | **Completed** — ladder fidelity→bcc→source→folder→policy + `decided_by` + All Custodians; D-0075-* residual |
| 4 | **0076** Tier-2 hardening | **Completed** — char-clamp, split-only guards, BoundBy, Tier 2.5 body/body-recip, `--dedupe-scope` (closes D-0075-scope); Codex luna PASS WITH DEFERRED P3 |
| 5 | **0079** Performance | **Completed** — Codex luna PASS WITH DEFERRED P3. Phases 0–5; `--jobs` skipped (D-0079-operator-multigb). |
| 6 | **0080** Output QC | **Completed** — Codex luna PASS WITH DEFERRED P3 — source-differential QC + `fidelity_contract_v1` allowlist; risk-weighted deterministic sample; scanpst `-no repair` (closes **D-0068-02** automatable half) + optional libpff/libpst counts-only sidecar; **COM declined** (new Outlook has no object model; classic EOL 2029). Folds into 0078 `VERIFY_FAILED` — no new exit integers. Contract-before-default-on gate; default `--qc-level sample` after fixture matrix green |
| 7 | **0081** Deps + runbook | Can draft early; finalize after 0073/0077 |

## Series M — Unique export fidelity residuals (0082–0087+)

| Order | Track | Notes |
|---|---|---|
| 1 | **0082** RecipientTableFidelity | **Completed** — recipient TC read+write + Tier-2.5 SMTP/EX |
| 2 | **0083** PromoteOnAttachFail | **Completed** — Mode A pre-write promote |
| 3 | **0084** NamedPropCloudAttach | **Completed** — attach-table cloud detect + pointer preserve |
| 4 | **0085** BodyCloudLinks | **Completed** — body-inline document-shaped URLs; residual D-0085-sovereign |
| 5 | **0086** AttachContentIdentity | **Completed** — `body-recip-attach` + Choice B unread |
| 6 | **0087** DeterministicStoreRecordKey | **Completed** — deterministic PidTagRecordKey / CoC volume digests; closes D-0079-deterministic-key |

**Series M closed through 0092.** **0090**/**0091** closed D-0086-embedded-email-hash / D-0086-digest-probe-unify. D-0073-eml and D-0085-sovereign-cloud-hosts closed in **0089** / **0088**.

## Series N — Operator fidelity (INC0102784 post-0092)

| Order | Track | Notes |
|---|---|---|
| 1 | **0093** WriterHeapRecipientRobustness | Land uncommitted heap diversion (cumulative/adaptive; 2048 documented deviation); Strategy **B** locked (budget-aware cap, To>Cc>Bcc, KnownGap) |
| 2 | **0094** EmbeddedMsgNestedExport | Highest attach soft-fail ROI — nested extract + PtypObject discovery; stop hardcoding `embedded_message: None` |
| 3 | **0095** UniquePstFolderTreeNormalize | Layout alias strip + lazy Unique Mail; close D-0070 pre-seed; QC fail classified from existing CSV |
| 3b | **0097** BodyCloudTruncationHonesty | **Completed** — C+A hybrid + split window_capped; closes D-0097 |
| 4 | **0096** PermissionTypeExtract | **Completed** — four-crate extract; low signal on INC* (0 attach-table cloud) |

**Suggested:** 0093 → 0094 → 0095 → 0096 → **0097** (all Completed). Series N closed.

## Series N+ — Verify count (0098)

| Order | Track | Notes |
|---|---|---|
| 1 | **0098** TemplateNidFolderCollision | **Completed** — skip reserved template nidIndex; INC* 4005 vs 4055 |

## Series P — Unique-PST defensibility (0099–0104)

| Order | Track | Notes |
|---|---|---|
| 1 | **0099** CrcPolyExportRiskHonesty | **Completed** — effective (non-poly) CRC rate for `export_risk`; D-0077-systematic-poly honesty half |
| 2 | **0100** RecipientTcMultipage | **Completed** — Strategy A (row-matrix subnode + RowsPerBlock + multi-block HN); D-0093-recipient-tc-multipage |
| 3 | **0101** EmbeddedDepthFlag | **Completed** — unique-pst `--max-embedded-depth` (default 3, reject outside 1–8); D-0067 CLI half; PR #92 |
| 4 | **0102** ExportOracleInputsAttest | **Completed** — remove recursive `"inputs"` strip; keep root `/inputs` blanking; 0099 pointers actually compare; D-0099-oracle-inputs-attest |
| 5 | **0103** RecipientTcSlblockNidOrder | **Completed** — trailing push + `add_subnode_leaf` emit-sort; D-0100-slblock-nid-order; PR #96 `f66ae9b` |
| 6 | **0104** AttachmentTcMultipage | **Completed** — Strategy A attach table (`0x671`); D-0093-attachment-tc-page |

**Suggested:** **0099**–**0104** **Completed**. No BCC track. Frontend Series O → **0110+**.

## Series Q — Unique-export honesty residuals (0105–0106)

| Order | Track | Notes |
|---|---|---|
| 1 | **0105** BodyCloudWindowEdgeNormalize | **Completed** — window-edge bare dedupe uses `normalize_candidate`; over-length joins `seen`; D-0097-window-edge-normalize |
| 2 | **0106** UniqueEmlNestedMime | **Completed** — unique-eml nested MIME from NestedCanonicalMessage; skip MAPI dump labeled rfc822; `--max-embedded-depth` on unique-eml; narrows D-0067 (do not close) |

## Series R — Unique-export operator co-export (0107)

| Order | Track | Notes |
|---|---|---|
| 1 | **0107** UniquePstAlsoEml | **Completed** — wire `unique-pst --also-eml` from the same keep-set (no second scan); closes D-0071-also-eml; PR #104 / `339dfa0`; Series S **0108–0109**; frontend **0110+** |

## Series S — Unique-export HITL residuals (0108–0109)

| Order | Track | Notes |
|---|---|---|
| 1 | **0108** PolyDegradedWinnerRisk | **Completed** — effective degraded_winner_rate; closes D-0108-poly-degraded-winner-risk; residual D-0108-keepset-crc-retaint |
| 2 | **0109** AlsoEmlClassifyHonesty | **Completed** — PR #104 Bugbot classify/cancel/counts; D-0109-also-eml-classify; PR #109 / `dc7c29c` |

## Series O — Review chrome Tauri 2 + Leptos (0110–0122)

**Series O** after Series S. **0110–0122 Completed**. **0123 Completed**. **0124 Completed**. **0125–0126 Proposed** (mockup fidelity; 0123 = Plex + navy, Home under bar).

| Order | Track | Notes |
|---|---|---|
| 1 | **0110** MatterChromeTauri | **Completed** — matter list/home; `matter_overview`; Plex/paper; PR **#111** `5a76f0b` |
| 2 | **0111** ReviewQueueFirstPass | **Completed** — virtualized first-pass queue; PR **#113** `3c4ca65` |
| 3 | **0112** ReviewWindow | **Completed** — three-pane; Resp ⊥ Privilege; PR **#115** `81a3aad` |
| 4 | **0113** ProduceChecklist | **Completed** — checklist; DAT only; no OPT; PR **#117** / `f192b2d` |
| 5 | **0114** PdfRasterRedact | **Completed** — zpdf CPU raster + geometric burn; schema v40; PR **#119** / `5ed53bf` |
| 6 | **0115** ImageOptFactory | **Completed** — TIFF G4 + OPT; page-level Bates; PR **#121** / `19d0c1f` |
| 7 | **0116** ProcessFold | **Completed** — swallow egui Process; process-runner + D-0113-long-job; PR **#123** / `727c857` |
| 8 | **0117** QueueVirtualizationResiduals | **Completed** — PR **#125** / `199975c`; header sibling, vacant honesty, arrow scrollTop |
| 9 | **0118** ReviewWindowAsyncResiduals | **Completed** — PR **#127** / `74fd797`; stale fetch guard + same-item codes refresh |
| 10 | **0119** ProduceChecklistResiduals | **Completed** — PR **#129** / `6a775b5`; empty `Some([])` log; Finalize latch; matter QC reset; success only on `succeeded` |
| 11 | **0120** PdfRasterUiResiduals | **Completed** — PR **#131** / `e87f4c1`; frame coords, draw cancel, Burn-set recount |
| 12 | **0121** ImageOptQcResiduals | **Completed** — PR **#135** / `600d6b3`; OPT skip until complete, scoped QC, sniff magic-first |
| 13 | **0122** ProcessFoldResiduals | **Completed** — PR **#137** / `f1810fe`; extract-all Busy keep-queue; live row Pause |

## Series T — Mockup chrome fidelity (0123–0126)

After **0119–0122** Bugbot. Steal layout from `C:\dev\dedupe-frontend`; do not vendor coral. **0123** before **0124–0126**. **0125** ∥ **0126** after shell.

| Order | Track | Notes |
|---|---|---|
| 1 | **0123** MatterShell | **Completed** — TopBar/StatusBar; Home under bar; Plex + ink-navy; recents BOM; PR **#139** / `fce416e` |
| 2 | **0124** ReviewQueueChrome | **Completed** — ellipsis; 244px rail; Go-to + row range; PR **#141** / `ff8b0ea` |
| 3 | **0125** ProduceCanvas | **Proposed** — five steps + Stage; not 0119 |
| 4 | **0126** ProcessChromeVisual | **Proposed** — jobs table; not 0122 |
