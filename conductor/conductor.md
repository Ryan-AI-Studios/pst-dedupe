# Dedupe Desk / pst-dedupe — Conductor Track Registry

Track registry for **Dedupe Desk** (local-first eDiscovery workstation) and the existing
`pst-dedupe` foundation crates. Tracks use the coordinated template convention
(`####-PascalDescription/`, `spec.md` + `plan.md`, Definition of Done in `spec.md`).

- **Execution repo:** `C:\dev\dedupe` (unless a track says otherwise)
- **Governance:** this directory (`C:\dev\dedupe\conductor\`)
- **Plan-of-record:** `C:\dev\Dedupe-plan.md`
- **Template:** `templates/0000-Description/`
- **Sequencing:** [`sequencing.md`](sequencing.md)
- **Guardrails:** [`TRACK-GUARDRAILS.md`](TRACK-GUARDRAILS.md)

## Status legend

`Ready` (spec/plan written, can start) · `In Progress` · `Blocked` · `Completed` · `Proposed` (backlog) · `Active` (legacy)

## Adding a track

1. Copy `templates/0000-Description/` → `####-PascalDescription/` (next free 4-digit id ≥ 0015 for Desk work).
2. Fill `spec.md` (objective, scope, preconditions, risks, **DoD**) and `plan.md` (phases → DoD).
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
| [track011-pst-writer-eml-import](track011-pst-writer-eml-import/plan.md) | **Active** | PST writer / EML import fixtures |
| — | **Completed** | Track 012 (reader crypt/HN/TC) — see git/history notes in `Dedupe-plan` / prior board |
| — | **Completed** | Track 013 (`pst-dedup-cli`) |
| — | **Completed** | Track 014 (docs refresh) |

Legacy folders keep `plan.md`/`spec.md`/`tdd.md` as written. **New work uses `####-PascalName`.**

---

## Series A — Foundation (MVP spine — Ready)

| Track | Status | Summary |
|---|---|---|
| [0015-MatterStore](0015-MatterStore/spec.md) | **Ready** | `matter-core` crate: SQLite + physical SHA-256 CAS + audit hash chain + jobs/checkpoints + item_errors |
| [0016-PurviewIngest](0016-PurviewIngest/spec.md) | **Ready** | Purview package/ZIP ingest + safety |
| [0017-NormalizedItem](0017-NormalizedItem/spec.md) | **Ready** | Canonical item model + attachment family |
| [0018-PstExtractorAdapter](0018-PstExtractorAdapter/spec.md) | **Ready** | pst-reader → Normalized Items |
| [0019-ProcessJobRunner](0019-ProcessJobRunner/spec.md) | **Ready** | In-app jobs; no external daemons |
| [0020-DeskShellUx](0020-DeskShellUx/spec.md) | **Ready** | Single-exe matter/source/process UX |

## Series B — Reduce & promote

| Track | Status | Summary |
|---|---|---|
| [0021-MatterDedupeJob](0021-MatterDedupeJob/spec.md) | **Ready** | Tiered dedupe as matter job |
| [0022-EmailThreading](0022-EmailThreading/spec.md) | **Proposed** | Email threading |
| [0023-NearDuplicateDetection](0023-NearDuplicateDetection/spec.md) | **Proposed** | Near-dup detection |
| [0024-CullAndReduce](0024-CullAndReduce/spec.md) | **Proposed** | Cull filters / reduction presets |
| [0025-PromoteToReview](0025-PromoteToReview/spec.md) | **Ready** | Promote-to-review corpus |

## Series C — Review core

| Track | Status | Summary |
|---|---|---|
| [0026-ReviewListViewer](0026-ReviewListViewer/spec.md) | **Ready** | Review list + email viewer |
| [0027-CodingAndBatch](0027-CodingAndBatch/spec.md) | **Ready** | Coding/tags + batch |
| [0028-FiltersSavedSearch](0028-FiltersSavedSearch/spec.md) | **Proposed** | Filters + saved searches |
| [0029-KeywordFtsSearch](0029-KeywordFtsSearch/spec.md) | **Proposed** | Keyword FTS |
| [0030-NotesHighlights](0030-NotesHighlights/spec.md) | **Proposed** | Notes / highlights |
| [0031-PrivilegeWorkflow](0031-PrivilegeWorkflow/spec.md) | **Proposed** | Privilege workflow + log |
| [0032-RedactionV1](0032-RedactionV1/spec.md) | **Proposed** | Redaction v1 |

## Series D — File types & OCR

| Track | Status | Summary |
|---|---|---|
| [0033-OfficeExtractors](0033-OfficeExtractors/spec.md) | **Proposed** | Office Open XML extractors |
| [0034-PdfExtractPreview](0034-PdfExtractPreview/spec.md) | **Proposed** | PDF extract + preview |
| [0035-CalendarItems](0035-CalendarItems/spec.md) | **Proposed** | Calendar items |
| [0036-OcrPlugin](0036-OcrPlugin/spec.md) | **Proposed** | Optional local OCR |
| [0037-FileCategoryTaxonomy](0037-FileCategoryTaxonomy/spec.md) | **Proposed** | File category taxonomy |

## Series E — Production & reporting

| Track | Status | Summary |
|---|---|---|
| [0038-CaseOverviewDashboard](0038-CaseOverviewDashboard/spec.md) | **Proposed** | Case overview dashboard |
| [0039-ProgressReporting](0039-ProgressReporting/spec.md) | **Proposed** | Progress / matter reports |
| [0040-ProductionExport](0040-ProductionExport/spec.md) | **Proposed** | Production export |
| [0041-ProductionQcRules](0041-ProductionQcRules/spec.md) | **Proposed** | Production QC rules |
| [0042-GapAnalysis](0042-GapAnalysis/spec.md) | **Proposed** | Gap analysis |

## Series F — Automation

| Track | Status | Summary |
|---|---|---|
| [0043-ProcessingProfiles](0043-ProcessingProfiles/spec.md) | **Proposed** | Processing profiles |
| [0044-WorkflowEngine](0044-WorkflowEngine/spec.md) | **Proposed** | Workflow engine (Rampiva-style) |
| [0045-CliAutomationParity](0045-CliAutomationParity/spec.md) | **Proposed** | CLI automation parity |

## Series G — Intelligence & optional AI

| Track | Status | Summary |
|---|---|---|
| [0046-EntityPiiPacks](0046-EntityPiiPacks/spec.md) | **Proposed** | Entity / PII packs |
| [0047-PeopleCommsGraph](0047-PeopleCommsGraph/spec.md) | **Proposed** | People–comms graph |
| [0048-ClusteringConceptMining](0048-ClusteringConceptMining/spec.md) | **Proposed** | Clustering / concept mining |
| [0049-SentimentNlpPlugin](0049-SentimentNlpPlugin/spec.md) | **Proposed** | Sentiment NLP plugin |
| [0050-SemanticSearchPlugin](0050-SemanticSearchPlugin/spec.md) | **Proposed** | Semantic search plugin |
| [0051-AiProviderTrait](0051-AiProviderTrait/spec.md) | **Proposed** | AI provider trait (opt-in) |
| [0052-AiReviewCitations](0052-AiReviewCitations/spec.md) | **Proposed** | AI review + citations |
| [0053-TranscriptionPlugin](0053-TranscriptionPlugin/spec.md) | **Proposed** | Transcription plugin |
| [0054-MultilingualPacks](0054-MultilingualPacks/spec.md) | **Proposed** | Multilingual packs |

## Series H — Teams / hard ESI

| Track | Status | Summary |
|---|---|---|
| [0055-TeamsChatAdapters](0055-TeamsChatAdapters/spec.md) | **Proposed** | Teams/chat adapters |
| [0056-ConversationReviewUi](0056-ConversationReviewUi/spec.md) | **Proposed** | Conversation review UI |

## Series I — Platform / SaaS

| Track | Status | Summary |
|---|---|---|
| [0057-SecurityHardener](0057-SecurityHardener/spec.md) | **Proposed** | Encryption at rest / desk security |
| [0058-MultiUserMatterService](0058-MultiUserMatterService/spec.md) | **Proposed** | Multi-user matter service |
| [0059-MultiTenantSso](0059-MultiTenantSso/spec.md) | **Proposed** | Multi-tenant + SSO |
| [0060-MultiJurisdictionProduction](0060-MultiJurisdictionProduction/spec.md) | **Proposed** | Multi-jurisdiction production |
| [0061-CloudBlobJobBackends](0061-CloudBlobJobBackends/spec.md) | **Proposed** | Cloud blob/job backends |

---

## Notes

- **Plan-of-record:** `C:\dev\Dedupe-plan.md` owns product architecture; this registry owns track lifecycle.
- **Template source:** structure aligned with `C:\dev\coordinated\conductor\templates\0000-Description\`.
- **MVP slice to implement first:** `0015 → 0016 → 0017 → 0018 → 0019 → 0020`, then `0021 → 0025 → 0026 → 0027` (see sequencing).
- **Ledgerful** is the provenance tool; `.ledgerful/` is gitignored.
- Historical ChangeGuard wording in legacy tracks is archival only.
