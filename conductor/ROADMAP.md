# Dedupe Desk — High-level track roadmap

> **Plan-of-record:** `C:\dev\Dedupe-plan.md`  
> **Registry:** [`conductor.md`](conductor.md) · **Order:** [`sequencing.md`](sequencing.md) · **Deferred:** [`../docs/deferred.md`](../docs/deferred.md)  
> **Features & UI map:** [`features.md`](features.md) · **How to use:** [`How-to-use.md`](How-to-use.md)  
> **Guardrails:** [`TRACK-GUARDRAILS.md`](TRACK-GUARDRAILS.md)

This file is the **placeholder + notes** layer for tracks that are not fully specified yet. When a track is scheduled for implementation, expand its `spec.md` / `plan.md` from the coordinated template and keep this summary in sync.

---

## Evidence & fixtures policy (hard rule)

### Never commit client or case evidence

| Allowed in git | Forbidden in git |
|---|---|
| Small **synthetic** / vendor fixtures under `fixtures/` (Aspose samples, generated zips, dummy `mail.pst` bytes) | Real case PSTs (e.g. Desktop `INC*.pst`), client exports, production Purview packages |
| Paths/docs that say “point Desk at a **local** path” | Copies under `evidence/`, `Matters/`, `output/`, Desktop mirrored into the repo |
| Redacted counts/logs in `review.md` if needed | Full subjects, bodies, or mailbox dumps in commits or public issues |

**Local smoke (recommended layout on your machine only):**

```text
C:\Users\<you>\Desktop\          # real PSTs stay here (or another encrypted volume)
C:\dev\dedupe\output\matters\    # local matters (gitignored via output/)
C:\dev\dedupe\fixtures\          # synthetic only — safe to commit
```

Desk/CLI should always accept **absolute local paths**. Automated tests use `fixtures/` + `tempfile` only.

**If a track needs multi-mailbox scale:** document optional **manual** smoke steps that point at operator-local files (e.g. `INC0102784*.pst` on Desktop). CI and git never require those files.

### CI / DoD implication

- DoD tests = synthetic fixtures + unit/integration under `tempfile`.
- Manual smoke on real PSTs = optional operator step in `review.md`, listed as deferred human smoke if not run (see D-0020-01 pattern).

---

## Status legend

| Status | Meaning |
|---|---|
| **Completed** | Spec/plan/review done; shipped |
| **Ready** | Spec/plan enough to start implementation |
| **Proposed** | Placeholder notes only; expand before coding |
| **MVP** | Needed for P0 counsel path |

---

## Series A — Foundation (MVP spine)

| ID | Track | Status | Notes |
|---|---|---|---|
| 0015 | MatterStore | **Completed** | SQLite + CAS + audit + jobs |
| 0016 | PurviewIngest | **Completed** | ZIP safety, leaf resume; synthetic `fixtures/purview/` |
| 0017 | NormalizedItem | **Completed** | Schema v2, family, logical_hash v1 + BCC |
| 0018 | PstExtractorAdapter | **Completed** | extract-pst; native v1; mid-folder resume |
| 0019 | ProcessJobRunner | **Completed** | Single matter worker; watch; Option C |
| 0020 | DeskShellUx | **Completed** | `dedupe-desk`; no client PST in repo |

**Done.** Local acceptance already proven on real multi-mailbox PSTs via CLI; keep those files **outside** the repo.

---

## Series B — Reduce & promote (next MVP)

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0021** | MatterDedupeJob | **Completed** | P0 done | MID → logical_hash job; flag-only; family policy; schema v3. |
| **0022** | EmailThreading | **Completed** | P1 done | `thread_id`; schema v4; job `thread`. |
| **0023** | NearDuplicateDetection | **Completed** | P1 done | MinHash shingles + LSH; pivot/similarity; schema v5; job `neardup`; CJK n-grams; no KM double-hash. |
| **0024** | CullAndReduce | **Completed** | P1 Wave 2 done | Flag-only cull presets; schema v6; job `cull`; feeds **0025**. Spec: `0024-CullAndReduce/`. |
| **0025** | PromoteToReview | **Completed** | P0 done | Review corpus membership (`in_review`); policy `auto` = cull_included ∥ unique_only; family expand default; schema v7; job `promote`. Spec: `0025-PromoteToReview/`. |

**MVP slice:** **0021–0056 Completed** (Series H closed). **0057** security hardener **Completed** (schema v35). **0058** multi-user matter service **Completed** (schema v36). Next: **0059** Multi-tenant + SSO.

---

## Series C — Review core (MVP gate)

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0026** | ReviewListViewer | **Completed** | **P0 done** | Review corpus list + body viewer; fixed-height virtualization; keyboard next/prev. |
| **0027** | CodingAndBatch | **Completed** | **P0 done** | Schema v8 catalog + `item_codes`; batch add/remove; full-id audit; whole-family opt-in. |
| **0028** | FiltersSavedSearch | **Completed** | **P1 done** | Schema v9 `saved_searches` + FilterSpec (flat AND); family CTE; code/date filters; Load more + list index; desk bar. |
| **0029** | KeywordFtsSearch | **Completed** | **P1 done** | Tantivy **0.26.x** per-matter `index/`; job `fts_index`; Boolean/phrase; ∩ with 0028 filters; delete-before-add + Windows mmap rebuild. Spec: `0029-KeywordFtsSearch/`. |
| **0030** | NotesHighlights | **Completed** | **P1 done** | Schema v11 notes + stand-off highlights (whitespace re-resolve); Review panel; filter has_notes/has_highlights/note_text. Spec: `0030-NotesHighlights/`. |
| **0031** | PrivilegeWorkflow | **Completed** | **P1 done** | Schema v12 `item_privilege` + withhold holds + standard privilege log CSV; matter 502 protocol stub; family split QC thin. Spec: `0031-PrivilegeWorkflow/`. |
| **0032** | RedactionV1 | **Completed** | **P1 done** | Schema v13 text redaction regions + true redacted CAS text (`[REDACTED]`); blackout paint; privilege partial_redaction hook; Codex luna PASS WITH DEFERRED P3. Spec: `0032-RedactionV1/`. |

**MVP gate (plan §7):** matter + extract + dedupe + promote + tag + basic export path. Real PST smoke stays **local only**.

---

## Series D — File types & OCR

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0033** | OfficeExtractors | **Completed** | **P1 done** | Schema v14 + `extract-office` DOCX/XLSX/PPTX text → CAS; job `office_extract`; zip/XML limits; synthetic `fixtures/office/`; Codex luna PASS WITH DEFERRED P3. Spec: `0033-OfficeExtractors/`. |
| **0034** | PdfExtractPreview | **Completed** | **P1 done** | Schema v15 + `extract-pdf` text → CAS; job `pdf_extract`; empty/low-text → `pdf_needs_ocr`; preview deferred (no pure-Rust raster); Codex luna PASS WITH DEFERRED P3. Spec: `0034-PdfExtractPreview/`. |
| **0035** | CalendarItems | **Completed** | **P1 done** | Schema v16 + `extract-calendar` multi-event container; extract-pst calendar class; job `ics_extract`; Codex luna PASS WITH DEFERRED P3. Spec: `0035-CalendarItems/`. |
| **0036** | OcrPlugin | **Completed** | **P2 done** | Schema v17 + `ocr-plugin`; opt-in Tesseract CLI + mock; job `ocr`; desk enable/Run OCR; consumes `pdf_needs_ocr`. Spec: `0036-OcrPlugin/`. |
| **0037** | FileCategoryTaxonomy | **Completed** | **P1 done** | Schema v18 + `file-category` `taxonomy_v1` classifier; job `classify`; bare `attachment` retired as category; Codex luna PASS. Spec: `0037-FileCategoryTaxonomy/`. |

---

## Series E — Production & reporting

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0038** | CaseOverviewDashboard | **Completed** | **P1 done** | Schema v19 + `load_case_overview` concurrent fan-out; desk Overview KPIs/tables (top-level size, review progress, errors-by-code); Codex luna PASS. Spec: `0038-CaseOverviewDashboard/`. |
| **0039** | ProgressReporting | **Completed** | **P1 done** | Exportable `matter_report_v1` CSV pack from `CaseOverview` + jobs; desk Export; audit; PDF optional pure-Rust. Spec: `0039-ProgressReporting/`. |
| **0040** | ProductionExport | **Completed** | **P1 done** | Produce volume: natives + text + Concordance DAT; withhold + redacted-text gates; schema v20; job `produce`. Spec: `0040-ProductionExport/`. |
| **0041** | ProductionQcRules | **Completed** | **P1 done** | Schema v21 + `matter-qc` pre-produce QC pack; findings CSV; `qc_runs` fingerprint gate; job `qc`; desk Run QC. Spec: `0041-ProductionQcRules/`. |
| **0042** | GapAnalysis | **Completed** | **P2 done** | Schema v22 + `matter-gap`: roster + week/month date holes; opposing DAT import + email-aware set-diff; desk Gap panel. Spec: `0042-GapAnalysis/`. |

---

## Series F — Automation

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0043** | ProcessingProfiles | **Completed** | **P1 done** | Schema v23 + `profile_run` child jobs; canonical stage order; cumulative reset; closes D-0036-04 / D-0037-07. Spec: `0043-ProcessingProfiles/`. |
| **0044** | WorkflowEngine | **Completed** | **P2 done** | Schema v24 + `workflow_run` + `parent_job_id`; AST bind; hard gates; sequential multi-job. Spec: `0044-WorkflowEngine/`. |
| **0045** | CliAutomationParity | **Completed** | **P1 done** | Headless matter CLI on `pst-dedup`; job/profile/workflow; closes D-0019-02 + Series E CLI rows. Spec: `0045-CliAutomationParity/`. |

---

## Series G — Intelligence & optional AI

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0046** | EntityPiiPacks | **Completed** | **P2 done** | Offline regex packs + Luhn; masked hits; `entity_scan`; schema v25. Spec: `0046-EntityPiiPacks/`. |
| **0047** | PeopleCommsGraph | **Completed** | **P2 done** | Relational `item_participants` + people/edges/timeline; job `people_graph`; schema v26. Spec: `0047-PeopleCommsGraph/`. |
| **0048** | ClusteringConceptMining | **Completed** | **P2 done** | Offline `tfidf_kmeans_v1` + c-TF-IDF labels; job `concept_cluster`; schema v27. Spec: `0048-ClusteringConceptMining/`. |
| **0049** | SentimentNlpPlugin | **Completed** | **P2 done** | Offline VADER-class `vader_lexicon_v1`; opt-in `sentiment` job; schema v28. Spec: `0049-SentimentNlpPlugin/`. |
| **0050** | SemanticSearchPlugin | **Completed** | **P2 done** | Local embeddings + chunk index; opt-in `semantic_index`; schema v29; FTS primary. Spec: `0050-SemanticSearchPlugin/`. |
| **0051** | AiProviderTrait | **Completed** | **P2 done** | Provider trait + first-pass code suggestions; schema v30; AI off by default. Spec: `0051-AiProviderTrait/`. |
| **0052** | AiReviewCitations | **Completed** | **P2 done** | Grounded citations + human promote UX; schema v31. Spec: `0052-AiReviewCitations/`. |
| **0053** | TranscriptionPlugin | **Completed** | **P3 done** | Local Whisper-class STT → CAS text; schema v32. Spec: `0053-TranscriptionPlugin/`. |
| **0054** | MultilingualPacks | **Completed** | **P2 done** | CJK FTS n-gram packs + fingerprint/rebuild; schema v33. Spec: `0054-MultilingualPacks/`. |

---

## Series H — Teams / hard ESI

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0055** | TeamsChatAdapters | **Completed** | **P2 done** | Teams HTML+PST→items + day-bucket conversation_id; schema v34. Spec: `0055-TeamsChatAdapters/`. |
| **0056** | ConversationReviewUi | **Completed** | **P2 done** | Conversation list + paged stream (day-bucket safe). Spec: `0056-ConversationReviewUi/`. |

---

## Series I — Platform / SaaS

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0057** | SecurityHardener | **Completed** | **P2 done** | Optional matter encryption (SQLite+CAS+FTS) + unlock UX; schema v35. Spec: `0057-SecurityHardener/`. |
| **0058** | MultiUserMatterService | **Completed** | **P3 done** | Opt-in local matter service + concurrent review (users, locks/batches, sampling QC); schema v36. Desk solo path remains default. Spec: `0058-MultiUserMatterService/`. |
| **0059** | MultiTenantSso | **Completed** | **P3 done** | Opt-in platform registry + tenant isolation + OIDC (PKCE); schema v37. Spec: `0059-MultiTenantSso/`. |
| **0060** | MultiJurisdictionProduction | **Completed** | **P3 done** | Named production profiles + QC packs; schema v38. Spec: `0060-MultiJurisdictionProduction/`. |
| **0061** | CloudBlobJobBackends | **Completed** | **P3 done** | Opt-in `BlobStore` (local + S3-compatible) + `JobBackend` trait; offline Desk default; schema v39. Spec: `0061-CloudBlobJobBackends/`. |

---

## Series J — Consolidation (post Series I)

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0062** | ReleaseHardeningRc | **Completed** | **P0 cons.** | RC 0.2.0-rc.1 freeze; SBOM/PDBs/deny/audit; handoff blocked on codesign. Spec: `0062-ReleaseHardeningRc/`. |
| **0063** | SecurityRedTeamFixes | **Completed** | **P0 cons.** | Time-boxed red team of 0057–0061 (+ light Series K path safety) + P0/P1 fixes. Spec: `0063-SecurityRedTeamFixes/`. |
| **0064** | DeskPlatformConnectUx | **Completed** | **P1 cons.** | Native egui Desk: Connect to matter-service (thin review + OCC), Solo produce profile + `bates_start`; soft SSO handoff. Closes D-0058-01 / D-0060-02. Spec: `0064-DeskPlatformConnectUx/`. |

**Order:** 0061 complete → **0062** RC → **0063** security → **0064** Desk UX (0063/0064 can partially overlap after 0062 freeze).

---

## Series K — Clean Unique export path (CLI-first)

> **Product goal:** Open 1..N → scan/inventory → integrity (recoverable vs skipped; **no source mutation**) → dedupe keep-set → unique EML (fast) and/or **unique PST** (hard) → single operator report pack.  
> **Reality:** scan + dedupe + report pieces exist; **production PST writer** is the major program. Fastest customer value = integrity + keep-set + unique EML + report; PST write is deliberate multi-track follow-on.  
> **Focus:** CLI always; Desk wizard optional (**0072**). Placeholders **Proposed**.

### Architecture

```text
PST₁…PSTₙ ──► Open / Scan (harden) ──► Integrity (no source mut)
                    │
                    ▼
              Dedupe keep-set (Tier1 MID / Tier2 hash; first-seen default)
                    │
        ┌───────────┼───────────┐
        ▼           ▼           ▼
   CSV/JSON    Unique EML   Unique PST writer
   report      (fast path)  (hard path)
```

### Pragmatic ship ladder

| Phase | Deliverable | PST? |
|---|---|---|
| **P0** | Multi-PST scan + CSV + skip reasons + unique EML pack | No |
| **P1** | Unique EML + import guide / optional Outlook automation | External |
| **P2** | Minimal unique PST (Unicode, flat, full body, basic props, limited attach) | Yes, limited |
| **P3** | Full fidelity unique PST (folders, attaches, multi-GB stream) | Yes |

### Tracks (A–H)

| ID | Letter | Track | Status | Priority | High-level notes |
|---|---|---|---|---|---|
| **0065** | A | ScanIntegrityReport | **Completed** | **P0 K** | Multi-PST integrity: skip reasons, recoverable vs skipped, thresholds, strict/best-effort. Spec: `0065-ScanIntegrityReport/`. |
| **0066** | B | DedupKeepSetExport | **Completed** | **P0 K** | Keep-set v1: first_seen/keep_largest/prefer_path, decision log, materialize winners, family attach policy, EDRM MIH. Spec: `0066-DedupKeepSetExport/`. |
| **0067** | C | UniqueEmlPackCli | **Completed** | **P0 K** | Unique EML pack from keep_set_v1: MIME multipart+stream attaches, eml_pack_v1 manifest. Spec: `0067-UniqueEmlPackCli/`. |
| **0068** | D | ProductionPstWriterV1 | **Completed** | **P0 K** | Production Unicode PST writer v1: IPM_SUBTREE + XBLOCK full body; Codex PASS WITH DEFERRED P3. Spec: `0068-ProductionPstWriterV1/`. |
| **0069** | E | PstWriterFidelity | **Completed** | **P0 K** | Attachments + folder preserve under IPM; Codex luna PASS. Spec: `0069-PstWriterFidelity/`. |
| **0070** | F | PstWriterStreamingScale | **Completed** | **P1 K** | Multi-GB streaming: AMap-aware, physical size, stop_and_finalize, hashes. Spec: `0070-PstWriterStreamingScale/`. |
| **0071 Completed** | **P0 K** | `unique-pst` CLI + unique_export_report_v1 + multi-volume + verify. Spec: `0071-CliUniquePstAndReport/`. |
| **0072** | H | DeskUniquePstWizard | **Completed** | **P2 opt.** | Optional GUI unique-pst wizard over `run_unique_pst` (pst-dedup-gui). Spec: `0072-DeskUniquePstWizard/`. |

**Order:**

```text
0065 integrity (A)
  └─► 0066 keep-set (B)
        ├─► 0067 unique EML (C) ──────────────┐
        └─► 0068 writer v1 (D)                │
              └─► 0069 fidelity (E)           ├─► 0071 CLI + report (G) ─► 0072 Desk (H opt)
                    └─► 0070 streaming (F) ───┘
```

### Effort reality

| Scope | Rough order |
|---|---|
| Scan + dedupe + report + unique EML (CLI) | **Weeks** (mostly glue + polish) |
| Competent unique PST v1 (limited fidelity) | **Months** |
| High-fidelity multi-GB unique PST | **Major program** (writer + verify + edges) |

### Locked product rules

- Source PSTs **read-only**; all “fixes” live in derived export  
- Default keep: Tier1 MID → Tier2 content hash, **first-seen wins**  
- v1 write: Unicode, unencrypted; no silent body truncate  
- Report must list **excluded** messages (legal/hold honesty)  
- Synthetic CI only; Outlook smoke operator-local

---

## Suggested implementation waves

### Wave 0 — Done
`0015`–`0020` foundation + Desk process path.

### Wave 1 — Unique set MVP
1. ~~**0021**–**0027**~~ reduce + promote + review + **tag** **done**  
2. Thin export/report if needed (subset of **0039/0040**)  

**Local-only acceptance:** Desktop multi-mailbox PSTs (never git) → matter → extract → dedupe → promote → review → tag.

### Wave 2 — Workstation (now)
1. ~~**0028** filters~~ **done**  
2. ~~**0029** Keyword FTS (Tantivy)~~ **done**  
3. ~~**0030** Notes / highlights~~ **done**  
4. ~~**0031** Privilege workflow / log~~ **done**  
5. ~~**0032** Redaction v1 (text)~~ **done**  
6. ~~**0033** Office extractors (OOXML)~~ **done**  
7. ~~**0034** PDF text extract (preview deferred)~~ **done**  
8. ~~**0035** Calendar items (PST/ICS)~~ **done**  
9. ~~**`0036` OCR plugin (opt-in local)**~~ **done**  
10. ~~**`0037` file category taxonomy**~~ **done**  
11. ~~**`0038` case overview dashboard**~~ **done**  
12. ~~**`0039` progress / matter reporting**~~ **done**  
13. ~~**`0040` production export**~~ **done**  
14. ~~**`0041` production QC**~~ **done**  
15. ~~**`0042` gap analysis**~~ **done**  
16. ~~**`0043` processing profiles**~~ **done**  
17. ~~**`0044` workflow engine**~~ **done**  
18. ~~**`0045` CLI automation**~~ **done** — Series F closed  
19. ~~**`0046` Entity/PII packs**~~ **done**  
20. ~~**`0047` People–comms graph**~~ **done**  
21. ~~**`0048` Concept clustering**~~ **done**  
22. ~~**`0049` Sentiment NLP**~~ **done**  
23. ~~**`0050` Semantic search**~~ **done**  
24. ~~**`0051` AI provider trait**~~ **done**  
25. ~~**`0052` AI review citations**~~ **done**  
26. ~~**`0053` Transcription plugin**~~ **done**  
27. ~~**`0054` Multilingual packs**~~ **done** — Series G closed  
28. ~~**`0055` Teams chat adapters**~~ **done**  
29. ~~**`0056` Conversation review UI**~~ **done** — Series H closed  
30. ~~Series I **`0057`** Security hardener~~ **done**  
31. ~~Series I **`0058`** Multi-user matter service~~ **done**  
32. ~~Series I **`0059`** Multi-tenant + OIDC SSO~~ **done**  
33. ~~Series I **`0060`** Multi-jurisdiction production profiles~~ **done**  
34. Series I **`0061`** Cloud blob/job backends (**Completed**) — Series I closed  
35. Series J **`0062`** RC hardening → **`0063`** red team → **`0064`** Desk Connect UX (**Completed**)
36. Series K **`0065`–`0072`** Clean Unique export path (**Completed**)
37. Series L **`0073`–`0081`** Unique export hardening (**Completed**)
38. Series M **`0082`–`0091`** Fidelity residuals (**Completed** through digest+probe unify); **`0092`** Ready

### Wave 3 — Plugins & platform
Intelligence / OCR packaging / AI / Teams (Series G–H), then Series I only if product commits to multi-user/cloud.

### Wave 4 — Consolidation (after 0061)
1. **0062** Release RC (no features)  
2. **0063** Security red team + fixes  
3. **0064** Desk Connect + produce profile UX  
4. Deferred triage + operator soak  

### Wave 5 — Clean Unique export (Series K)
1. **0065** (A) Scan integrity report  
2. **0066** (B) Dedup keep-set + materialization  
3. **0067** (C) Unique EML pack CLI ← **fastest customer value**  
4. **0068** (D) Production PST writer v1 (flat, full body) ← **long pole starts**  
5. **0069** (E) Attachments + folder preservation  
6. **0070** (F) Multi-GB streaming stress  
7. **0071** (G) CLI `unique-pst` + report pack + verify  
8. **0072** (H) Desk wizard (optional)

---

## Addendum — After track 0061 (Series I close)

> **When:** As soon as **0061** CloudBlobJobBackends is **Completed**.  
> **Why:** Tracks **0015–0061** cover the plan-of-record spine (workstation + platform hooks). The next phase is **consolidation**, not unbounded feature expansion.  
> **Deferred file:** [`docs/deferred.md`](../docs/deferred.md) is large (~400+ rows). Do **not** auto-schedule every residual as a track.

### Goal shift

| Before 0061 | After 0061 |
|---|---|
| Expand capability via Series A–I tracks | **Ship, prove, secure, make usable** |
| “What’s the next feature ID?” | “What blocks a boutique firm from using this?” |
| Defer freely into `deferred.md` | **Triage** deferred (close / polish / promote / park) |

### Default sequence (locked product guidance)

```text
0061 Completed
    │
    ▼
0062  Release hardening / RC          ← feature freeze; golden path; gates; version
    │
    ▼
0063  Security red team + P0/P1 fixes ← 0057–0061 surfaces only; time-boxed
    │
    ▼
0064  Desk Connect + produce UX       ← native egui; D-0058-01 / D-0060-02 (SSO UX if cheap)
    │
    ▼
Deferred triage + operator soak       ← real PSTs local-only; bugfix tracks only
    │
    ▼
Series K Clean Unique export (CLI)    ← planned product path (0065–0072 Proposed)
```

Series J: [`0062`](0062-ReleaseHardeningRc/spec.md)–[`0063`](0063-SecurityRedTeamFixes/spec.md) **Completed**; [`0064`](0064-DeskPlatformConnectUx/spec.md) **Completed**. Series K [`0065`](0065-ScanIntegrityReport/spec.md)–[`0072`](0072-DeskUniquePstWizard/spec.md) **Completed**. Series L (0073–0081 unique-export hardening) **Completed**. Series M: **0082**–**0091** Completed; **0092** Ready.

### Wave 6 — Unique export fidelity residuals (Series M)

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0082** | RecipientTableFidelity | **Completed** | **P0 M** | MS-PST recipient TC read+write (`0x692`); SMTP+EX Tier-2.5; BCC write opt-in + suppress ledger; `retryable`; closes D-0080/D-0076 recip + D-0078-retryable. |
| **0083** | PromoteOnAttachFail | **Completed** | **P0 M** | Mode A pre-write promote-on-attach-fail (`--promote-on-attach-fail`); closes **D-0073-promote**; Mode B declined; Codex luna PASS. |
| **0084** | NamedPropCloudAttach | **Completed** | **P0 M** | MS-PST NPMAP (`0x61`) + **attach-table** cloud detect; `ATTACH_CLOUD_LINK` + `cloud_url`/`cloud_provider` ledger; pointer preserve; Mode A incomplete; closes **D-0080** (detect≠hydrate); residual body-inline links; Codex luna PASS. |
| **0085** | BodyCloudLinks | **Completed** | **P0 M** | Body-inline **document-shaped** URL scan (`:x:` Excel incl.); hit-list CSV query-preserved; Mode A known gap documented; closes **D-0084-body-cloud-links**; residual **D-0085-sovereign-cloud-hosts** → **0088**; Codex luna PASS WITH DEFERRED P3. |
| **0086** | AttachContentIdentity | **Completed** | **P1 M** | Enable `body-recip-attach`: full-stream attach SHA-256; **Choice B** unread sentinels; fail-closed strict enum; NIST KAT; closes **D-0076-attach-content**; residual **D-0086-embedded-email-hash** / **D-0086-digest-probe-unify** → **0090**/**0091**; Codex luna PASS WITH DEFERRED P3. |
| **0087** | DeterministicStoreRecordKey | **Completed** | **P1 M** | Deterministic `PidTagRecordKey`/ProviderUID (length-prefix SHA-256 preimage; no wall-clock/PID/path); job-global seed + volume-layout honesty; cross-process + multi-volume key proofs; DoD-3 path A on fixture; closes **D-0079-deterministic-key**. Codex luna PASS. |
| **0088** | SovereignCloudHosts | **Completed** | **P2 M** | GCC High / DoD sovereign host allowlist + office365.us SafeLinks; closes **D-0085-sovereign-cloud-hosts**; residual **D-0088-usgovcloud-microsoft-tld**. Codex luna PASS WITH DEFERRED P3. |
| **0089** | UniqueEmlAttachLedger | **Completed** | **P1 M** | unique-eml attach ledger CSV via `EmlAttachEvent` DTO; closes **D-0073-eml**; Codex luna final PASS. Spec: `0089-UniqueEmlAttachLedger/`. |
| **0090** | EmbeddedMsgContentHash | **Completed** | **P2 M** | Bounded `embedded-msg-hash/v1` (not Relativity parity; method-5 subnode parse); closes **D-0086-embedded-email-hash**. Spec: `0090-EmbeddedMsgContentHash/`. |
| **0091** | DigestProbeUnify | **Completed** | **P2 M** | CLI record-don’t-tee (Pass-1 Real by-value digest seeds Full/ok; Pass-2 skip + one logical charge); closes **D-0086-digest-probe-unify**. Spec: `0091-DigestProbeUnify/`. Codex luna r3 PASS. |
| **0092** | CloudNamedPropWrite | **Ready** | **P2 M** | Allowlisted NPMAP + `PidNameAttachmentProviderType`; closes **D-0084-cloud-named-prop-write**. Spec: `0092-CloudNamedPropWrite/`. |

**Order:** 0088 ∥ 0089 → 0090 → 0091 → 0092.

### Wave 7 — Unique-PST defensibility (Series P)

Post-0098 INC* soak: verify counts match; handoff still not affidavit-clean. **0101 Completed** (PR **#92**). **0102 Completed** (oracle attest). **0103 Completed** (SLBLOCK NID order; PR **#96**). **0104 Completed** (attach-table Strategy A). **Do not** add a BCC-write track (0082 default suppress is the disclosure policy).

| ID | Track | Status | Priority | High-level notes |
|---|---|---|---|---|
| **0099** | CrcPolyExportRiskHonesty | **Completed** | **P0 P** | Dual-rate poly CRC must not elevate `export_risk`. Thresholds key on **effective** (non-poly) `block_crc_read_rate`; attach-stream CRC discounted only when every CRC-noisy source is poly-class. INC*: preflight `ok` vs `not_export_ready` (raw 1.0, 6014 events). Closes export-risk half of **D-0077-systematic-poly**; fingerprint residual **D-0077-poly-fingerprint**. Never in-tool repair. Spec: `0099-CrcPolyExportRiskHonesty/`. |
| **0100** | RecipientTcMultipage | **Completed** | **P0 P** | Strategy A: all included TC rows. Row-matrix subnode + RowsPerBlock packing + multi-block HN on the recipient-table node; shared `TableContext::load`. Closes **D-0093-recipient-tc-multipage**. BCC default unchanged. PR **#90**. Spec: `0100-RecipientTcMultipage/`. |
| **0101** | EmbeddedDepthFlag | **Completed** | **P1 P** | CLI `--max-embedded-depth` (reject outside 1–8, default 3). Same value to materialize + writer. CI proves 4@3 vs 4@4 and 8@7 vs 8@8. Narrows unique-pst **D-0067-embedded-depth** (row stays open). Identity hash depth stays 3. PR **#92**. Spec: `0101-EmbeddedDepthFlag/`. |
| **0102** | ExportOracleInputsAttest | **Completed** | **P2 P** | Recursive oracle `"inputs"` strip deleted `export_risk.inputs`. Removed that allowlist key; blank job-level `/inputs` at root only; keep 0099 pointers. Closes **D-0099-oracle-inputs-attest**. Spec: `0102-ExportOracleInputsAttest/`. |
| **0103** | RecipientTcSlblockNidOrder | **Completed** | **P2 P** | Trailing matrix `push` + `add_subnode_leaf` NID-ascending emit-sort; fail closed on duplicate NIDs. Closes **D-0100-slblock-nid-order**. PR **#96** `f66ae9b`. Spec: `0103-RecipientTcSlblockNidOrder/`. |
| **0104** | AttachmentTcMultipage | **Completed** | **P2 P** | Strategy A for per-message attachment table (`0x671`): row-matrix subnode + RowsPerBlock (`Floor(8176/25)=327`) + multi-block HN. Closes **D-0093-attachment-tc-page**. HNBITMAPHDR stay fail-closed. PR **#98** `a35927c`. Spec: `0104-AttachmentTcMultipage/`. |

**Order:** **0099** → **0100** → **0101** → **0102** → **0103** → **0104**. Frontend Series O (if started) uses **0105+**.

### What each step is for

| Step | Do | Do not |
|---|---|---|
| **0062 RC** | Tag/version, changelog, golden path (matter→produce offline), mode honesty (solo vs service vs SSO vs cloud **opt-in**), workspace gate green | New features, schema product work, “finish deferred” |
| **0063 Red team** | Threat model; adversarial tests; fix **P0/P1** on encrypt, multi-user, OIDC, produce integrity, cloud CAS; cargo audit/deny | FedRAMP program, infinite audit, all historical security residuals |
| **0064 Desk UX** | Connect to matter-service (thin review + OCC), session actor, Solo produce profile + Bates start; soft SSO handoff | WASM rewrite, full remote feature parity, remote produce/jobs API, portfolio multi-matter UI |
| **Triage + soak** | Close/never noise in deferred; promote 5–10 operator pains; local multi-mailbox smoke | Commit client PSTs |
| **Series K** | Integrity + keep-set + unique EML first; then multi-track **production PST writer** + report pack | In-place source “repair”; overclaim scanpst; silent fidelity loss; all-messages-in-RAM |

### Deferred triage buckets (use when gobbling residuals)

| Bucket | Action |
|---|---|
| **Close / never** | Document and drop (superseded or rejected product) |
| **Polish P3** | Batch only if cheap; else leave parked |
| **Operator pain** | Promote to a thin track (e.g. Connect, produce dropdown) |
| **Platform residual** | Park until a real multi-tenant/hosted customer (SAML, K8s fleet, CMK, multi-matter host) |
| **Market feature** | New series only with explicit buyer (image/OPT factory, deep Relativity, etc.) |

### Explicitly not next by default

- Uncritically implementing all of `docs/deferred.md`  
- Immediate FedRAMP / full SaaS ops / billing  
- Another intelligence/plugin series without demand  
- Image production factory, full Relativity suite, SCIM — unless production rejections or a paid requirement  
- Rewriting Desk as web/WASM (Desk remains **native eframe/egui**)

### Series K — Clean Unique export (planned)

**Chosen product story:** CLI path to unique set with honest integrity + keep-set provenance; **unique EML fast path**; **unique PST** as deliberate writer program (v1 → fidelity → multi-GB). See Series K table (**0065–0072**).

**Still optional later (not Series K):**

| Theme | Examples | Signal |
|---|---|---|
| Image production | TIFF/PDF + OPT/LFP (D-0040-01) | Buyers require image sets |
| Hosted firm | Multi-matter host, HTTP workers, CMK | Paid multi-tenant pilot |
| Interop depth | CP1252 DAT, slip sheets, richer load files | Productions rejected in the field |
| AI depth | Batch QC, privilege assist | AI is the wedge product |

**Locked product rules for Series K:** never mutate operator original PST; Class C in-place binary repair out of v1; honest partials + reason codes; synthetic CI only.

### UI stack reminder

| Surface | Stack |
|---|---|
| **Dedupe Desk** | Native Windows **`eframe` / `egui`** (`dedupe-desk`) — **not WASM** |
| **CLI** | `pst-dedup` headless |
| **Service** | `matter-service` HTTP (opt-in multi-user / platform) |

### Success criteria for “post-61 done enough”

1. RC build + golden path a non-conductor person can follow.  
2. Series I security P0s fixed or explicitly accepted.  
3. Multi-user path usable from Desk (Connect), not CLI-only.  
4. Deferred list reduced and prioritized — not grown by default.  
5. Next feature series (if any) named from **customer pain**, not empty track numbers.

---

## Track authoring checklist (when expanding a placeholder)

1. Copy depth from `templates/0000-Description/` or recent Ready tracks (0021 style).  
2. Cite plan-of-record sections + `docs/deferred.md` owners.  
3. State **fixture strategy**: synthetic in `fixtures/`; optional **local path** smoke only.  
4. Register status in `conductor.md` and order in `sequencing.md`.  
5. Never add steps that `git add` real PST paths.

---

## Related local (non-repo) references

| Path | Role |
|---|---|
| `C:\dev\Dedupe-plan.md` | Product plan-of-record |
| `C:\dev\Comparison.md` | Optional Nuix comparison |
| Operator Desktop / external volume | Real PST smoke only |
