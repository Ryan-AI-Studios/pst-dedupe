# Dedupe Desk / pst-dedupe â€” Conductor Track Registry

Track registry for **Dedupe Desk** (local-first eDiscovery workstation) and the existing
`pst-dedupe` foundation crates. Tracks use the coordinated template convention
(`####-PascalDescription/`, `spec.md` + `plan.md`, Definition of Done in `spec.md`).

- **Execution repo:** `C:\dev\dedupe` (unless a track says otherwise)
- **Governance:** this directory (`C:\dev\dedupe\conductor\`)
- **Plan-of-record:** `C:\dev\Dedupe-plan.md`
- **Template:** `templates/0000-Description/`
- **Sequencing:** [`sequencing.md`](sequencing.md)
- **High-level roadmap (placeholders + waves):** [`ROADMAP.md`](ROADMAP.md)
- **Guardrails:** [`TRACK-GUARDRAILS.md`](TRACK-GUARDRAILS.md)
- **Evidence policy:** real case PSTs stay **out of git** â€” see ROADMAP + `fixtures/`

## Status legend

`Ready` (spec/plan written, can start) Â· `In Progress` Â· `Blocked` Â· `Completed` Â· `Proposed` (backlog) Â· `Active` (legacy)

## Adding a track

1. Copy `templates/0000-Description/` â†’ `####-PascalDescription/` (next free 4-digit id â‰¥ 0015 for Desk work).
2. Fill `spec.md` (objective, scope, preconditions, risks, **DoD**) and `plan.md` (phases â†’ DoD).
3. Add a row below; set status `Ready` or `Proposed`.
4. On completion: write `review.md`, flip status to **Completed**, commit ledger in execution repo.

---

## Legacy foundation (pre-template)

| Track | Status | Summary |
|---|---|---|
| [track001-infra-baseline-gates](track001-infra-baseline-gates/spec.md) | **Completed** | Workspace compile + baseline gates |
| [track002-real-pst-fixtures-traversal](track002-real-pst-fixtures-traversal/spec.md) | **Completed** | Real PST fixtures + traversal |
| [track003-dedup-tier-semantics](track003-dedup-tier-semantics/spec.md) | **Completed** | Tier 1/2 dedupe semantics |
| [track004-gui-errors-partial-results](track004-gui-errors-partial-results/spec.md) | **Completed** | GUI error/partial results |
| [track005-export-unique-eml](track005-export-unique-eml/spec.md) | **Completed** | Unique EML export |
| [track006-quality-gates-repair](track006-quality-gates-repair/spec.md) | **Completed** | Quality gate config repair |
| [track007-docs-readme-architecture](track007-docs-readme-architecture/spec.md) | **Completed** | README + architecture |
| [track008-pst-reader-hardening](track008-pst-reader-hardening/spec.md) | **Completed** | Reader hardening / CRC |
| [track009-windows-release-packaging](track009-windows-release-packaging/spec.md) | **Completed** | Windows release packaging |
| [track010-audit-hardening](track010-audit-hardening/plan.md) | **Completed** | Audit security fixes |
| [track011-pst-writer-eml-import](track011-pst-writer-eml-import/plan.md) | **Completed** | Fixture writer exit criteria met; production writer is 0068+ (archival close in 0062) |
| â€” | **Completed** | Track 012 (reader crypt/HN/TC) â€” see git/history notes in `Dedupe-plan` / prior board |
| â€” | **Completed** | Track 013 (`pst-dedup-cli`) |
| â€” | **Completed** | Track 014 (docs refresh) |

Legacy folders keep `plan.md`/`spec.md`/`tdd.md` as written. **New work uses `####-PascalName`.**

---

## Series A â€” Foundation (MVP spine â€” Ready)

| Track | Status | Summary |
|---|---|---|
| [0015-MatterStore](0015-MatterStore/spec.md) | **Completed** | `matter-core` crate: SQLite + physical SHA-256 CAS + audit hash chain + jobs/checkpoints + item_errors |
| [0016-PurviewIngest](0016-PurviewIngest/spec.md) | **Completed** | `ingest-purview`: detect + safe ZIP expand + leaf checkpoints + encoding fallbacks |
| [0017-NormalizedItem](0017-NormalizedItem/spec.md) | **Completed** | Schema v2 Normalized Item + family graph + logical_hash v1 (BCC-aware) |
| [0018-PstExtractorAdapter](0018-PstExtractorAdapter/spec.md) | **Completed** | `extract-pst`: PST â†’ Normalized Items + native v1 + mid-folder resume |
| [0019-ProcessJobRunner](0019-ProcessJobRunner/spec.md) | **Completed** | `process-runner`: single matter worker, watch progress, Option C `*_on_job`, Drop join |
| [0020-DeskShellUx](0020-DeskShellUx/spec.md) | **Completed** | Single-exe matter/source/process UX (`dedupe-desk`) |

## Series B â€” Reduce & promote

| Track | Status | Summary |
|---|---|---|
| [0021-MatterDedupeJob](0021-MatterDedupeJob/spec.md) | **Completed** | MID + logical_hash matter job; family flags; schema v3 |
| [0022-EmailThreading](0022-EmailThreading/spec.md) | **Completed** | thread_id: header graph + subject fallback; extract In-Reply-To/References; job `thread` (P1 Wave 2) |
| [0023-NearDuplicateDetection](0023-NearDuplicateDetection/spec.md) | **Completed** | MinHash shingles + LSH clusters; pivot/similarity; job `neardup`; schema v5 (P1 Wave 2) |
| [0024-CullAndReduce](0024-CullAndReduce/spec.md) | **Completed** | Flag-only cull presets (unique/date/path/type/empty; optional local DeNIST); job `cull`; schema v6 |
| [0025-PromoteToReview](0025-PromoteToReview/spec.md) | **Completed** | Review corpus: auto cull_includedâˆ¥unique_only; family expand; job `promote`; schema v7 |

## Series C â€” Review core

| Track | Status | Summary |
|---|---|---|
| [0026-ReviewListViewer](0026-ReviewListViewer/spec.md) | **Completed** | in_review list + CAS body viewer; family strip; keyboard next/prev; thin rows + off-thread body |
| [0027-CodingAndBatch](0027-CodingAndBatch/spec.md) | **Completed** | Schema v8 code catalog + item_codes; batch add/remove + audit full ids; whole-family opt-in; desk coding UI |
| [0028-FiltersSavedSearch](0028-FiltersSavedSearch/spec.md) | **Completed** | Schema v9 saved_searches + FilterSpec SQL; family CTE; desk filter bar + Load more; Codex luna PASS WITH DEFERRED P3 |
| [0029-KeywordFtsSearch](0029-KeywordFtsSearch/spec.md) | **Completed** | Tantivy 0.26.x + fts_index; keyword âˆ© FilterSpec; Codex luna PASS WITH DEFERRED P3 |
| [0030-NotesHighlights](0030-NotesHighlights/spec.md) | **Completed** | Schema v11 notes/highlights + whitespace re-resolve; desk panel + filters; Codex luna PASS WITH DEFERRED P3 |
| [0031-PrivilegeWorkflow](0031-PrivilegeWorkflow/spec.md) | **Completed** | Schema v12 privilege claims + withhold + log CSV; Codex luna PASS WITH DEFERRED P3 |
| [0032-RedactionV1](0032-RedactionV1/spec.md) | **Completed** | Schema v13 text redaction + true redacted CAS `[REDACTED]`; desk blackout; Codex luna PASS WITH DEFERRED P3 |

## Series D â€” File types & OCR

| Track | Status | Summary |
|---|---|---|
| [0033-OfficeExtractors](0033-OfficeExtractors/spec.md) | **Completed** | Schema v14 + extract-office DOCX/XLSX/PPTX text â†’ CAS; job `office_extract`; Codex luna PASS WITH DEFERRED P3 |
| [0034-PdfExtractPreview](0034-PdfExtractPreview/spec.md) | **Completed** | Schema v15 + extract-pdf text â†’ CAS; job `pdf_extract`; empty/low_text â†’ `pdf_needs_ocr`; no pure-Rust preview; Codex luna PASS WITH DEFERRED P3 |
| [0035-CalendarItems](0035-CalendarItems/spec.md) | **Completed** | Schema v16 + extract-calendar ICS container; extract-pst calendar class; job `ics_extract`; Codex luna PASS WITH DEFERRED P3 |
| [0036-OcrPlugin](0036-OcrPlugin/spec.md) | **Completed** | Schema v17 + ocr-plugin Tesseract CLI OCR; job `ocr`; Codex luna PASS WITH DEFERRED P3 |
| [0037-FileCategoryTaxonomy](0037-FileCategoryTaxonomy/spec.md) | **Completed** | Schema v18 + file-category taxonomy_v1 + job `classify`; Codex luna PASS |

## Series E â€” Production & reporting

| Track | Status | Summary |
|---|---|---|
| [0038-CaseOverviewDashboard](0038-CaseOverviewDashboard/spec.md) | **Completed** | Schema v19 indexes + `load_case_overview` concurrent fan-out; desk Overview KPIs/tables; top-level size, review progress, errors-by-code; Codex luna PASS |
| [0039-ProgressReporting](0039-ProgressReporting/spec.md) | **Completed** | matter_report_v1 CSV pack from CaseOverview + jobs; desk Export; audit; open_for_read + scrub; PDF deferred D-0039-01; Codex luna PASS |
| [0040-ProductionExport](0040-ProductionExport/spec.md) | **Completed** | Schema v20 + matter-produce: natives+text+Concordance DAT (BOM/®/UTC/FILE_EXT); withhold+redacted gates; job `produce`; desk Produce; Codex luna PASS |
| [0041-ProductionQcRules](0041-ProductionQcRules/spec.md) | **Completed** | Schema v21 + `matter-qc`: default QC pack, findings CSV, `qc_runs` fingerprint, produce gate, job `qc`, desk Run QC; Codex luna FAIL→fix rounds + final independent PASS WITH DEFERRED P3 |
| [0042-GapAnalysis](0042-GapAnalysis/spec.md) | **Completed** | Expected custodians + date coverage; opposing DAT import + email-aware set-diff; schema v22; Codex auth blocked → independent PASS WITH DEFERRED P3 |

## Series F â€” Automation

| Track | Status | Summary |
|---|---|---|
| [0043-ProcessingProfiles](0043-ProcessingProfiles/spec.md) | **Completed** | Named stage presets + sequential `profile_run`; schema v23; Codex luna FAIL→fix rounds + final gate; unblocks 0044 |
| [0044-WorkflowEngine](0044-WorkflowEngine/spec.md) | **Completed** | Declarative multi-node `workflow_run` + `parent_job_id`; schema v24; AST bind; hard gates; Codex luna FAIL→PASS + final gate; unblocks 0045 |
| [0045-CliAutomationParity](0045-CliAutomationParity/spec.md) | **Completed** | Headless matter CLI on `pst-dedup`: job/profile/workflow run; closes D-0019-02 + Series E CLI deferreds; shared `register_default_handlers` |

## Series G â€” Intelligence & optional AI

| Track | Status | Summary |
|---|---|---|
| [0046-EntityPiiPacks](0046-EntityPiiPacks/spec.md) | **Completed** | Offline regex entity/PII packs + Luhn; masked hits; `entity_scan` job; schema v25; Codex luna FAIL→fix + final gate |
| [0047-PeopleCommsGraph](0047-PeopleCommsGraph/spec.md) | **Completed** | People–comms graph: relational participants + directed edges + timeline; schema v26; job `people_graph` |
| [0048-ClusteringConceptMining](0048-ClusteringConceptMining/spec.md) | **Completed** | Concept clustering: offline `tfidf_kmeans_v1` + c-TF-IDF labels; schema v27; job `concept_cluster` |
| [0049-SentimentNlpPlugin](0049-SentimentNlpPlugin/spec.md) | **Completed** | Offline VADER-class sentiment (`vader_lexicon_v1`); opt-in job; schema v28 |
| [0050-SemanticSearchPlugin](0050-SemanticSearchPlugin/spec.md) | **Completed** | Local embeddings semantic search (chunk index); opt-in; schema v29; FTS remains primary |
| [0051-AiProviderTrait](0051-AiProviderTrait/spec.md) | **Completed** | AI provider trait (off by default) + first-pass code suggestions; schema v30 |
| [0052-AiReviewCitations](0052-AiReviewCitations/spec.md) | **Completed** | AI citations + human promote UX; schema v31; grounded quotes |
| [0053-TranscriptionPlugin](0053-TranscriptionPlugin/spec.md) | **Completed** | Local STT transcription plugin; schema v32; Mock+Whisper CLI |
| [0054-MultilingualPacks](0054-MultilingualPacks/spec.md) | **Completed** | Multilingual packs: CJK FTS n-gram + pack fingerprint; schema v33 |

## Series H â€” Teams / hard ESI

| Track | Status | Summary |
|---|---|---|
| [0055-TeamsChatAdapters](0055-TeamsChatAdapters/spec.md) | **Completed** | Teams/chat export adapters (HTML+PST→items); schema v34; day-bucket conversation_id |
| [0056-ConversationReviewUi](0056-ConversationReviewUi/spec.md) | **Completed** | Conversation review UI: day-bucket list + paged stream |

## Series I â€” Platform / SaaS

| Track | Status | Summary |
|---|---|---|
| [0057-SecurityHardener](0057-SecurityHardener/spec.md) | **Completed** | Matter encryption at rest (DB+CAS+FTS) + unlock UX; schema v35; pure-Rust AEAD |
| [0058-MultiUserMatterService](0058-MultiUserMatterService/spec.md) | **Completed** | Multi-user matter service (opt-in HTTP host + locks + sampling QC); schema v36; Codex luna PASS WITH DEFERRED P3 |
| [0059-MultiTenantSso](0059-MultiTenantSso/spec.md) | **Completed** | Multi-tenant isolation + OIDC SSO (platform.db; schema v37; Codex luna PASS) |
| [0060-MultiJurisdictionProduction](0060-MultiJurisdictionProduction/spec.md) | **Completed** | Multi-jurisdiction production profiles + QC packs (schema v38; Codex luna PASS WITH DEFERRED P3) |
| [0061-CloudBlobJobBackends](0061-CloudBlobJobBackends/spec.md) | **Completed** | Opt-in BlobStore (S3) + JobBackend trait; offline local default; schema v39; Codex luna PASS |

## Series J — Consolidation (post Series I)

| Track | Status | Summary |
|---|---|---|
| [0062-ReleaseHardeningRc](0062-ReleaseHardeningRc/spec.md) | **Completed** | RC freeze 0.2.0-rc.1: CHANGELOG, golden paths, audit/deny, SBOM+PDBs+ZIP; Codex luna PASS WITH DEFERRED P3; handoff blocked on D-0062-codesign |
| [0063-SecurityRedTeamFixes](0063-SecurityRedTeamFixes/spec.md) | **Completed** | Series I red team + P0/P1 fixes (encrypt/service/SSO/produce/cloud) on RC freeze |
| [0064-DeskPlatformConnectUx](0064-DeskPlatformConnectUx/spec.md) | **Completed** | Desk Connect + thin remote review (OCC) + Solo produce profile/Bates; SSO loopback; Codex luna PASS WITH DEFERRED P3 |

## Series K — Clean Unique export path (CLI-first)

| Track | Status | Summary |
|---|---|---|
| [0065-ScanIntegrityReport](0065-ScanIntegrityReport/spec.md) | **Completed** | (A) Multi-PST scan integrity: reason codes, modes, streaming ledger, preflight; Codex luna PASS WITH DEFERRED P3 |
| [0066-DedupKeepSetExport](0066-DedupKeepSetExport/spec.md) | **Completed** | (B) Keep-set v1: fidelity-before-policy, promote-on-mat-fail, decision stream, EDRM MIH; Codex luna PASS WITH DEFERRED P3 |
| [0067-UniqueEmlPackCli](0067-UniqueEmlPackCli/spec.md) | **Completed** | (C) Unique EML pack from keep_set_v1 — MIME multipart + manifest eml_pack_v1; Codex luna PASS |
| [0068-ProductionPstWriterV1](0068-ProductionPstWriterV1/spec.md) | **Completed** | (D) Production Unicode PST writer v1 — IPM_SUBTREE + Deleted Items/Search Root + fixed MS-PST template objects, full body XBLOCK, no silent truncate; Codex luna 11-round PASS WITH DEFERRED P3 |
| [0069-PstWriterFidelity](0069-PstWriterFidelity/spec.md) | **Completed** | (E) Attachments (by-value/XBLOCK) + folder path preserve under IPM_SUBTREE; Codex luna PASS |
| [0070-PstWriterStreamingScale](0070-PstWriterStreamingScale/spec.md) | **Completed** | (F) Multi-GB streaming — AMap-aware, eager spill, chunked attach, physical size + stop_and_finalize, SHA-256/MD5; Claude final PASS WITH DEFERRED P3 |
| [0071-CliUniquePstAndReport](0071-CliUniquePstAndReport/spec.md) | **Completed** | (G) CLI unique-pst + unique_export_report_v1 + multi-volume + verify |
| [0072-DeskUniquePstWizard](0072-DeskUniquePstWizard/spec.md) | **Completed** | (H) Optional GUI unique-pst wizard over run_unique_pst; Codex luna PASS |

---


## Series L - Unique export hardening (post-0072 / INC0102784 lessons)

Operator evidence (2026-07-26): multi-mailbox `INC0102784.pst` + `-2.pst` -> unique-pst **3728** msgs, **366** attach fails, ~**108k** page CRC warns, **~275 s** export. Source PSTs stay out of git.

| Track | Status | Summary |
|---|---|---|
| [0073-ExportAttachmentFailureLedger](0073-ExportAttachmentFailureLedger/spec.md) | **Completed** | Attach ledger: locus+source_id, reason taxonomy, CSV injection safe, row-cap, non-blocking sink; Codex luna PASS WITH DEFERRED P3 (D-0073-*) |
| [0074-DeepAttachPreflightFidelity](0074-DeepAttachPreflightFidelity/spec.md) | **Completed** | Budgeted L2 deep attach preflight; opt-in scan+unique-pst; peer-cap + unprobed degrade; cache→stream_available; strict rebuild; Codex luna PASS WITH DEFERRED P3 (D-0074-*) |
| [0075-KeepSetWinnerPolicies](0075-KeepSetWinnerPolicies/spec.md) | **Completed** | Winner ladder: earliest_date, BCC, folder-class, source-rank, `decided_by`, All Custodians; graded fidelity opt-in; defaults inert; Codex luna FAIL→fix + final gate; D-0075-* residual |
| [0076-ContentHashTierHardening](0076-ContentHashTierHardening/spec.md) | **Completed** | Identity binding: char-clamp panic fix, unread/degenerate + cross-MID guards, BoundBy, Tier 2.5 body/body-recip (attach rejected→D-0076-attach-content), `--dedupe-scope per-source` (closes D-0075-scope); Codex luna PASS WITH DEFERRED P3 |
| [0077-CrcNoiseAndExportRisk](0077-CrcNoiseAndExportRisk/spec.md) | **Completed** | Integrity telemetry in the **data path** (not a log Layer): bounded CRC emission with exact totals, per-source `page/block_crc_mismatches` + `distinct_bad_bids`, `block_crc_rate` + `block_crc_read_rate` (the metrics `crc_skip_rate` was blind to), **`CRC_SUSPECT` item taint** so CRC-corrupt-but-parseable bodies stop poisoning 0076 Tier-2 identity, `export_risk` on the **existing** `PreflightRecommendation` vocabulary (advisory vs catastrophic tiers), Desk wizard risk banner; rolls in D-0074-crc-fixture + D-0073-vec-events; ScanPST-deletes-items count-diff / Purview physical-vs-logical runbook |
| [0078-UniqueExportExitCodes](0078-UniqueExportExitCodes/spec.md) | **Completed** | Automation contract: `run → Result<CliExit>`, `classify_export`, exit **64**/`65`/`130`, cancel quarantine, JSON fidelity contract; Codex luna PASS WITH DEFERRED P3 (D-0078-retryable/gui; D-0073-eml narrowed not closed) |
| [0079-MaterializeWritePerformance](0079-MaterializeWritePerformance/spec.md) | **Completed** | Measurement-gated perf: `PhaseTimings` + counters; export equivalence oracle (not byte-reproducible — D10); single-materialize + by-value convert; O(1) AMap; positioned writes; shared `PstHandleCache` LRU (closes D-0074-mat-lru); concurrent hash; **`--jobs` not shipped** (fixture residual sub-second; multi-GB residual D-0079-operator-multigb); Codex luna PASS WITH DEFERRED P3 |
| [0080-UniquePstOutlookQc](0080-UniquePstOutlookQc/spec.md) | **Completed** | **Source-differential** output QC: today's verify compares the output to the *same run's report* (self-referential) and never reads attachments or the folder tree back. Adds machine-readable `fidelity_contract_v1` (allowlist — unknown property fails closed) separating `known_gap` from `defect`/`unexplained_loss`; risk-weighted deterministic sampling (XBLOCK bodies, volume seams, per-source, longest subject → D-0068-01); promotes 0079 `structural_digest_pst` out of test-only. **Outlook COM declined** (no object model in new Outlook — default since April 2026, classic EOL 2029); **scanpst `-no repair`** on a local copy closes the automatable half of **D-0068-02**; optional libpff/libpst counts-only sidecar (process, never linked) breaks reader/writer circularity; human-signed `qc_attestation_v1`. Surfaces silently-dropped `display_cc`/`display_bcc` |
| [0081-UniqueExportDepsAndOperatorDocs](0081-UniqueExportDepsAndOperatorDocs/spec.md) | **Completed** | Dep pin audit + eDiscovery runbook + `--ledger-path-mode` + timing script; Codex luna PASS WITH DEFERRED P3 |

**Suggested order:** 0073 -> 0074 (fidelity) // 0077 (noise) // 0078 (exits) ; then 0075 policies; 0076 hash; 0079 perf; 0080 QC; 0081 docs anytime after 0073/0077 facts land. **Series L closed** (0073–0081 Completed).

**Maps prior improvement list:**
1 attach ledger -> 0073 | 2 deep preflight -> 0074 | 3 first_seen/date -> 0075 | 4 tier2 -> 0076 | 5 folder class -> 0075 | 6 near-dup -> use existing 0022/0023 matter jobs (not re-opened here) | 7 CRC noise -> 0077 | 8 exit codes -> 0078 | 9 export risk -> 0077 | 10 multi-volume exists (0070/0071) | 11 Outlook QC -> 0080 | perf -> 0079 | deps/docs -> 0081

## Series M — Unique export fidelity residuals (post–Series L)

After 0073–0081, the highest-value deferred cluster is **structured MAPI recipients** (read + write + identity), which three deferreds already name as one track. Later Series M tracks can take Mode A promote, named props / cloud attach, deterministic record keys, etc.

| Track | Status | Summary |
|---|---|---|
| [0082-RecipientTableFidelity](0082-RecipientTableFidelity/spec.md) | **Completed** | Read/write recipient TC (MS-PST MUST `0x692`); SMTP+EX Tier-2.5 identity; BCC write opt-in + `bcc_suppressed`; `retryable` summary; closes D-0080/D-0076 recip + D-0078-retryable; Codex luna PASS |
| [0083-PromoteOnAttachFail](0083-PromoteOnAttachFail/spec.md) | **Completed** | Mode A pre-write promote (`--promote-on-attach-fail`); Mode C default; Mode B declined; closes **D-0073-promote**; `winner_promoted` honesty; Codex luna PASS |
| [0084-NamedPropCloudAttach](0084-NamedPropCloudAttach/spec.md) | **Completed** | MS-PST NPMAP (`0x61`) resolve + **attachment-table** cloud detect; `ATTACH_CLOUD_LINK` + ledger `cloud_url`/`cloud_provider`; pointer preserve (anti-ghost); Mode A incomplete; closes **D-0080** detect≠hydrate; residual body-inline links; Codex luna PASS |
| [0085-BodyCloudLinks](0085-BodyCloudLinks/spec.md) | **Completed** | Body-inline **document-shaped** SharePoint/OneDrive URL scan (`:w:/:x:/:p:…`); `export_body_cloud_links.csv` (query preserved); closes **D-0084-body-cloud-links**; Mode A non-interaction + known gap; sovereign residual **D-0085-sovereign-cloud-hosts**; Codex luna PASS WITH DEFERRED P3 |
| [0086-AttachContentIdentity](0086-AttachContentIdentity/spec.md) | **Completed** | Tier-2.5 **`body-recip-attach`**: full-stream per-attach SHA-256 + Choice B unread sentinels; fail-closed enum; closes **D-0076-attach-content**; Codex luna PASS WITH DEFERRED P3 |
| [0087-DeterministicStoreRecordKey](0087-DeterministicStoreRecordKey/spec.md) | **Completed** | Deterministic `PidTagRecordKey` / ProviderUID (length-prefix SHA-256 preimage; no wall-clock/PID/path); cross-process + multi-volume key proofs; DoD-3 path A on fixture; closes **D-0079-deterministic-key**; Codex luna PASS |

**Suggested order:** 0082–**0087** Completed.

## Series M (continued) — Unique export fidelity residuals (0088–0092)

Promoted from deferred (2026-08-24). **0088–0092 Completed**.

| Track | Status | Summary |
|---|---|---|
| [0088-SovereignCloudHosts](0088-SovereignCloudHosts/spec.md) | **Completed** | GCC High / DoD sovereign SharePoint+OneDrive host allowlist + `*.safelinks.protection.office365.us` (closes **D-0085-sovereign-cloud-hosts**; residual **D-0088-usgovcloud-microsoft-tld**); Codex luna PASS WITH DEFERRED P3 |
| [0089-UniqueEmlAttachLedger](0089-UniqueEmlAttachLedger/spec.md) | **Completed** | unique-eml `export_attachments.csv` parity via `EmlAttachEvent` DTO (closes **D-0073-eml**); Codex luna final PASS |
| [0090-EmbeddedMsgContentHash](0090-EmbeddedMsgContentHash/spec.md) | **Completed** | Bounded `embedded-msg-hash/v1` (not Relativity parity) for method-5 subnode + rfc822 under body-recip-attach (closes **D-0086-embedded-email-hash**) |
| [0091-DigestProbeUnify](0091-DigestProbeUnify/spec.md) | **Completed** | CLI record-don’t-tee: Pass-1 Real by-value digest seeds Full/ok; Pass-2 skips re-stream + charges once (closes **D-0086-digest-probe-unify**); Codex luna r3 PASS |
| [0092-CloudNamedPropWrite](0092-CloudNamedPropWrite/spec.md) | **Completed** | Allowlisted NPMAP write + `PidNameAttachmentProviderType` (closes **D-0084-cloud-named-prop-write**); Codex luna r6 PASS WITH DEFERRED P3 (**D-0092-permission-type-extract**) |

**Suggested order:** 0088 ∥ 0089 → 0090 → 0091 → 0092 (writer; optional until counsel needs Outlook named-prop visibility).

## Series N — Operator fidelity (INC0102784 post-0092)

Promoted from operator unique-pst smoke on Desktop `INC0102784.pst` + `INC0102784-2.pst` (2026-08-25): 4055 msgs written; exit `VERIFY_FAILED`+`ATTACH_SOFT_FAIL`; 374/374 attach fails = `ATTACH_EMBEDDED_UNPARSED`; QC defects = `folder_tree_structure` + `recipient_table` (interim 48-row cap); body-cloud CSV 62 empty truncates vs 3 real links; attach-table cloud providers = 0 (0092 NPMAP not exercised on this corpus). Evidence under `output/inc0102784-0092-full/` (operator-local; not committed).

| Track | Status | Summary |
|---|---|---|
| [0093-WriterHeapRecipientRobustness](0093-WriterHeapRecipientRobustness/spec.md) | **Completed** | Strategy B + cumulative heap; closes **D-0068-01**; residuals **D-0093-recipient-tc-multipage** / **D-0093-attachment-tc-page**; Codex luna r4 PASS |
| [0094-EmbeddedMsgNestedExport](0094-EmbeddedMsgNestedExport/spec.md) | **Completed** | Method-5 nested export + PtypObject `0x3701`; closes **D-0069-embed-object**; narrows **D-0067**; Codex r5 PASS WITH DEFERRED P3 (INC* re-smoke) |
| [0095-UniquePstFolderTreeNormalize](0095-UniquePstFolderTreeNormalize/spec.md) | **Completed** | Leading alias strip + lazy Unique Mail + **D-0070 closed** via `known_source_paths`; QC key symmetry + Deleted Items claimable; Codex loop through r2 fixes |
| [0096-PermissionTypeExtract](0096-PermissionTypeExtract/spec.md) | **Completed** | Four-crate PermissionType extract (PtypInteger32 MAY); closes **D-0092-permission-type-extract**; QC live-read / cloud-pointer only; Codex r4 PASS |
| [0097-BodyCloudTruncationHonesty](0097-BodyCloudTruncationHonesty/spec.md) | **Completed** | C+A hybrid truncation honesty: `truncated` := dropped document-shaped candidates; ≤1 marker (`WINDOW` / `MAX_LINKS_EXCEEDED` / `URL_TRUNCATED`); split `body_scan_window_capped_messages`; closes **D-0097-body-cloud-truncate-honesty** |

**Suggested order:** **0093** → **0094** → **0095** → **0096** → **0097** (all Completed). Series N closed.

## Series N+ — Verify count (template NID collision)

| Track | Status | Summary |
|---|---|---|
| [0098-TemplateNidFolderCollision](0098-TemplateNidFolderCollision/spec.md) | **Completed** | Folder nidIndex `0x30` satellite TCs collided with MS-PST templates `0x60D`–`0x60F`; unique-pst verify **4005 vs 4055** (50 Purges orphans). `alloc_nid` skips reserved indices; duplicate NBT NID fail-closed. Closes **D-0098-template-nid-collision**. |

## Series P — Unique-PST defensibility (post-0098 INC* soak, 0099–0104)

Ranked for affidavit-grade unique-PST: CRC class first, then full recipient TCs, then nested-depth flag. **No BCC track** (0082 default suppress stays). Hermes Series O frontend, if started, uses **0108+**. **0101 Completed** (PR **#92** `4bbf620`). **0102 Completed** (oracle `export_risk.inputs` attest; PRs **#94** / **#95**). **0103 Completed** (recipient-table SLBLOCK NID order; PR **#96** `f66ae9b`). **0104 Completed** (attach-table TC Strategy A).

| Track | Status | Summary |
|---|---|---|
| [0099-CrcPolyExportRiskHonesty](0099-CrcPolyExportRiskHonesty/spec.md) | **Completed** | Dual-rate poly CRC must not elevate unique-pst `export_risk`; thresholds key on effective (non-poly) `block_crc_read_rate`. INC* preflight `ok` vs `not_export_ready` (`block_crc_read_rate=1.0`, 6014 `ATTACH_STREAM_CRC`). Promotes **D-0077-systematic-poly** honesty half. Fingerprint residual. Never in-tool repair. |
| [0100-RecipientTcMultipage](0100-RecipientTcMultipage/spec.md) | **Completed** | Strategy A: full included recipient TC. Row-matrix subnode + RowsPerBlock + multi-block HN on the table node; shared `TableContext::load`. Closes **D-0093-recipient-tc-multipage**. BCC default unchanged. PR **#90** `ab1c7b0`. |
| [0101-EmbeddedDepthFlag](0101-EmbeddedDepthFlag/spec.md) | **Completed** | Wire `--max-embedded-depth` (clap reject outside 1–8, default 3) on unique-pst. Same value to materialize + writer. CI: 4@3 vs 4@4 and 8@7 vs 8@8. Narrows unique-pst half of **D-0067-embedded-depth** (row stays open). Identity hash depth stays 3. PR **#92** `4bbf620`. HITL INC* at depth 8 skipped. |
| [0102-ExportOracleInputsAttest](0102-ExportOracleInputsAttest/spec.md) | **Completed** | Recursive oracle `"inputs"` strip deleted `export_risk.inputs` so 0099 attest pointers never compared. Removed `"inputs"` from `SUMMARY_ALLOWLIST_KEYS`; blank job-level `/inputs` at root only. Closes **D-0099-oracle-inputs-attest**. |
| [0103-RecipientTcSlblockNidOrder](0103-RecipientTcSlblockNidOrder/spec.md) | **Completed** | Trailing matrix `push` + `add_subnode_leaf` NID-ascending emit-sort (fail closed on duplicates). Closes **D-0100-slblock-nid-order**. PR **#96** `f66ae9b`. |
| [0104-AttachmentTcMultipage](0104-AttachmentTcMultipage/spec.md) | **Completed** | Strategy A for per-message attachment table (`0x671`): row-matrix subnode + RowsPerBlock + multi-block HN. Closes **D-0093-attachment-tc-page**. HNBITMAPHDR stay fail-closed. PR **#98** `a35927c`. Frontend Series O starts **0110+**. |

**Suggested order:** **0099** → **0100** → **0101** → **0102** → **0103** → **0104**.

## Series Q — Unique-export honesty residuals (post-0104)

Series P closed. **0105–0106 Completed.** **No BCC track.** Frontend Series O, if started, uses **0110+**.

| Track | Status | Summary |
|---|---|---|
| [0105-BodyCloudWindowEdgeNormalize](0105-BodyCloudWindowEdgeNormalize/spec.md) | **Completed** | Window-edge bare dedupe runs `normalize_candidate` before classify; over-length URLs join `seen`. Closes **D-0097-window-edge-normalize**. Not frontend. |
| [0106-UniqueEmlNestedMime](0106-UniqueEmlNestedMime/spec.md) | **Completed** | unique-eml reconstructs RFC 5322 inside `message/rfc822` from 0094 `NestedCanonicalMessage`; skip MAPI dump; `--max-embedded-depth` 1–8 default 3. Narrows **D-0067-embedded-depth** (do not close). Not frontend. |

**Suggested order:** Series Q **0105–0106 Completed**. Frontend Series O uses **0110+**.

## Series R — Unique-export operator co-export (post-0106)

Series Q closed. unique-eml nested MIME is honest (0106), so `unique-pst --also-eml` can write the same keep-set as EML. **No BCC track.** Frontend Series O uses **0110+**.

| Track | Status | Summary |
|---|---|---|
| [0107-UniquePstAlsoEml](0107-UniquePstAlsoEml/spec.md) | **Completed** | Wire `unique-pst --also-eml` to a unique-eml pack from the **same** keep-set (no second scan). Closes **D-0071-also-eml**. Not frontend. PR [#104](https://github.com/Ryan-AI-Studios/pst-dedupe/pull/104) / `339dfa0`. |

**Suggested order:** **0108–0109** Completed. Frontend Series O **0110+**.

## Series S — Unique-export HITL residuals (post-0107)

Structural INC* HITL **2026-08-29** is green (4055/4055, depth 8, also-eml). **0108–0109** Completed (effective degrade rate + also-eml classify/cancel honesty). **No BCC track.** Frontend **0110+**.

| Track | Status | Summary |
|---|---|---|
| [0108-PolyDegradedWinnerRisk](0108-PolyDegradedWinnerRisk/spec.md) | **Completed** | Effective `degraded_winner_rate` excludes poly-only `CrcSuspect`/`AttachStreamCrc` on poly-class sources. HITL: 3931 CRC-only vs 124 also `BODY_UNAVAILABLE` → effective ≈ 0.031 (still advisory; stops the 1.000 lie). Closes **D-0108-poly-degraded-winner-risk**. Not frontend. |
| [0109-AlsoEmlClassifyHonesty](0109-AlsoEmlClassifyHonesty/spec.md) | **Completed** | Combined also-eml fidelity/ok/cancel rewrite honesty (PR #104 Bugbot). Closes **D-0109-also-eml-classify**. Not frontend. PR [#109](https://github.com/Ryan-AI-Studios/pst-dedupe/pull/109) / `dc7c29c`. |

**Suggested order:** Series S **0108–0109** Completed. Frontend **0110+**.

## Series O — Review chrome (Tauri 2 + Leptos)

**Timing (2026-08-29):** unique-export Series S is closed. This is the **next Dedupe series**. **0110 In progress** (Phases 0–3 implement). 0111–0116 stay Proposed (0115 parked). Do not remint IDs. Do not vendor `C:\dev\dedupe-frontend`.

**Stack lock:** Tauri 2 + Leptos, Plex/paper/cool chrome, keep egui Process until **0116**. One pipeline. No daemon. Image/OPT **0115** parked until a produce needs images. Search builder folds into **0111** (no 0117). No BCC-default track.

| Track | Status | Summary |
|---|---|---|
| [0110-MatterChromeTauri](0110-MatterChromeTauri/spec.md) | **In progress** | Tauri 2 + Leptos matter list/home; one `matter_overview` command (`load_case_overview`). Plex/paper. egui Process stays. |
| [0111-ReviewQueueFirstPass](0111-ReviewQueueFirstPass/spec.md) | **Proposed — placeholder** | Virtualized first-pass queue; lead/QC is a second view. |
| [0112-ReviewWindow](0112-ReviewWindow/spec.md) | **Proposed — placeholder** | Three-pane coding; Responsiveness ⊥ Privilege; Native/Text; Image stub. |
| [0113-ProduceChecklist](0113-ProduceChecklist/spec.md) | **Proposed — placeholder** | Mock checklist wired to `matter-qc` + `matter-produce`. DAT only. No OPT. |
| [0114-PdfRasterRedact](0114-PdfRasterRedact/spec.md) | **Proposed — placeholder** | `zpdf` raster + geometric redact; pdfium fallback. Feeds **D-0032-01** / **D-0034-02**. |
| [0115-ImageOptFactory](0115-ImageOptFactory/spec.md) | **Proposed — parked** | TIFF G4 + OPT. Only if a produce needs images. **D-0040-01**. |
| [0116-ProcessFold](0116-ProcessFold/spec.md) | **Proposed — placeholder** | Swallow egui Process into the same Tauri window. Still one pipeline. |

**Suggested order:** **0110** → **0111** → **0112** → **0113**; **0114** after window; **0115** parked; **0116** last. Do not skip to raster/OPT first.

## Notes

- **Plan-of-record:** `C:\dev\Dedupe-plan.md` owns product architecture; this registry owns track lifecycle.
- **Roadmap placeholders:** [`ROADMAP.md`](ROADMAP.md) â€” waves, priorities, **evidence policy** (no client PSTs in git).
- **Template source:** structure aligned with `C:\dev\coordinated\conductor\templates\0000-Description\`.
- **MVP slice:** Series A–H Completed; Series I **`0057`–`0061` Completed** (schema through **v39**; platform spine closed). Series K Clean Unique export: **0065–0072 Completed**. Series J consolidation: **0062 Completed** (RC `0.2.0-rc.1`); **0063 Completed** (security red team; D-0063-01..05 residual); **0064 Completed** (Desk Connect + Solo produce profile UX; D-0064-01..08 residual). Series L **0073–0081 Completed**. Series M **0082–0092 Completed** (Unique export fidelity residuals closed through allowlisted NPMAP write). Series N **0093–0097 Completed** (INC0102784 operator fidelity follow-ups). **0098 Completed** (template NID / verify −50). **Series P 0099–0104 Completed**. **Series Q 0105–0106 Completed** (window-edge + unique-eml nested MIME). **Series R 0107 Completed** (`unique-pst --also-eml` co-export; PR #104 / `339dfa0`). **Series S 0108–0109 Completed** (poly degrade risk + also-eml classify). **Series O 0110 In progress** (Tauri 2 + Leptos chrome); **0111–0116 Proposed** (0115 parked).
- **Fixtures:** synthetic under `fixtures/` only; real multi-mailbox PSTs are **operator-local** smoke (Desktop/external), never committed.
- **Deferred memory:** `docs/deferred.md`.
- **Desk UI iteration (debug / cargo-watch):** [`ui-iteration.md`](ui-iteration.md).
- **Ledgerful** is the provenance tool; `.ledgerful/` is gitignored.
- Historical ChangeGuard wording in legacy tracks is archival only.
