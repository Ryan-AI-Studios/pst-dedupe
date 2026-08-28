# Deferred items (Dedupe)

Track-scoped findings and intentional product deferrals that are **not** blocking
completion, but must not be lost. Update when fixed or when a track owns the work.

## From track 0016-PurviewIngest (Codex / internal review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0016-01 | P3 | Nested zip open may re-increment `nested_zips` counter on resume | Telemetry only; expand correctness OK | future polish |
| D-0016-02 | P3 | ZIP general-purpose bit 11 approximated (not always read from raw flags) | Documented in `ingest-purview` README; encoding fallbacks still preserve names | future polish |
| D-0016-03 | P3 | No unique index on `items(source_id, path)` | App-level skip for resume; still optional after 0017 | see D-0017-01 |
| D-0016-04 | — | Streaming multi-GB single entry without full buffer | Buffer cap only in 0016 | later performance |
| D-0016-05 | — | 7z expand | Explicit `unsupported_7z` only | future track |
| D-0016-06 | — | PST message extract | **Done in 0018** (`extract-pst`) | — |
| D-0016-07 | — | Full Normalized Item model | **Done in 0017** | — |
| D-0016-08 | — | Blocking worker pool / process runner | **Done in 0019** (`process-runner`) | — |
| D-0016-09 | — | CLI `ingest` smoke subcommand | **Closed in 0045** (`pst-dedup ingest`) | — |

## From track 0017-NormalizedItem

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0017-01 | P3 | Unique index on `items(source_id, path)` | Optional in DoD; resume remains app-level | future polish |
| D-0017-02 | P3 | Formal SQLite FK on `parent_item_id` | App-enforced; ALTER cannot add FK cleanly | later migration if needed |
| D-0017-03 | — | Relational `item_participants` | **Done in 0047** (`item_participants` + people/edges/timeline schema v26) | — |
| D-0017-04 | — | Body-to-CAS promote helper (`text_sha256`) | **Done in 0018** (body → CAS + column) | — |
| D-0017-05 | — | Bulk rehash / fill from PST | Extract fill in 0018; runner in 0019 | bulk job polish later |

## From track 0018-PstExtractorAdapter

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0018-01 | P3 | Attach path may materialize large subnode `Vec` before stream switch | Primary path streams; residual fallback | future polish |
| D-0018-02 | — | EML as native identity | Never; **closed in 0040** (export-only EML packaging; not CAS identity) | — |
| D-0018-03 | — | MAPI recipient table (vs Display* only) | **Reader half closed in 0082** (`pst-reader` walks `NID_TYPE_RECIPIENT_TABLE`, surfaces `Recipient` / types / SMTP+EX). **Residual:** matter `extract-pst` path still builds participants from DisplayTo/Cc/Bcc only (`extract.rs` parse_display_list) — does not yet consume structured TC rows into `item_participants` | **closed / 0082** (reader); residual matter extract |
| D-0018-04 | — | Process runner / progress UI | Runner **done in 0019**; Desk UI progress | **0020** |

## From track 0019-ProcessJobRunner

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0019-01 | — | Multi-job parallel stages per matter | **Partial close in 0044**: sequential multi-job orchestration (`workflow_run` / `profile_run` child rows). **True parallel** stages remain residual (SQLite single-writer) | residual / **D-0044-02** |
| D-0019-02 | — | Full CLI `job run|resume|cancel` | **Closed in 0045** (`job run|resume|cancel|status|list` + profile/workflow) | — |
| D-0019-03 | P3 | Extract cancel→resume via runner | Ingest path proven; extract fixture success proven | future polish |
| D-0019-04 | — | Rayon pure-CPU stages without Matter | Forbidden for Matter path P0 | later |

| D-0018-05 | — | CLI `extract` subcommand | **Closed in 0045** via `job run --kind extract_pst` | — |

## From track 0020-DeskShellUx

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0020-01 | P3 | Human interactive GUI smoke (full click path) | Automated: 17 unit tests + release build + WAL concurrent read; smoke steps in crate README | operator / polish |
| D-0020-02 | — | Drag-drop / system theme / multi-window | Spec optional / not DoD | later |
| D-0018-04 | — | Process runner / progress UI | Runner 0019; **Desk UI done in 0020** | — |

## From track 0021-MatterDedupeJob

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0021-01 | — | Policy B (logical wins on MID conflict) | P0 is Policy A + `mid_logical_conflicts` | optional later |
| D-0021-02 | — | Near-duplicate / fuzzy match | **Done in 0023** (`matter-neardup` / `minhash_shingle_v1`) | — |
| D-0021-03 | — | Threading (conversation) | **Done in 0022** (`matter-thread`) | — |
| D-0021-04 | P3 | SQL GROUP BY / page family dup parents (multi-million scale) | Parent pass pages; family pass still lists thin parents then filters dups (Codex/internal P3) | scale polish |
| D-0021-05 | — | Cross-family attach link by native only when parents unique | Family pass only for duplicate parents | later if needed |
| D-0021-06 | P3 | Full GUI smoke for Run dedupe click path | Automated handler + unit tests; operator smoke local | operator / polish |
| D-0017-01 | P3 | Unique index on `items(source_id, path)` | Unchanged; 0021 keys by item id + MID/logical | future polish |

## From track 0022-EmailThreading

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0022-01 | — | Full JWZ container/message dual objects as review items | Item-centric model only; not needed for P0 | later / **0056** |
| D-0022-02 | — | Richer Outlook ConversationIndex tree (parse blocks) | Opaque 22-byte / 44-hex prefix only | later / **0056** |
| D-0022-03 | P3 | Full GUI smoke for Run threading click path | Automated handler + unit tests; operator smoke local | operator / polish |
| D-0022-04 | — | Optional thread-count badge after job complete | Spec optional; not DoD | later polish |
| D-0022-05 | P3 | Re-extract still skips body/attachment re-CAS on existing paths | Headers-only refresh by design (0022); full retry-with-update deferred | future extract polish |
| D-0017-01 | P3 | Unique index on `items(source_id, path)` | Unchanged | future polish |

## From track 0023-NearDuplicateDetection

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0023-01 | — | Deep email reply-quote stripping | P0 body-only CAS; prefer **0022** for threads | residual polish |
| D-0023-02 | — | Multi-million signature spill to SQLite | P0 holds signatures in memory (~128×8 B + id per doc) | scale polish |
| D-0023-03 | — | Optional gaoya / txtfp crates | In-crate MinHash P0 for auditability | optional later |
| D-0023-04 | P3 | Full GUI smoke for Run near-dup click path | Automated handler + unit tests; operator smoke local | operator / polish |
| D-0021-02 | — | Near-duplicate / fuzzy match | **Done in 0023** | — |
| D-0017-01 | P3 | Unique index on `items(source_id, path)` | Unchanged | future polish |

## From track 0024-CullAndReduce

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0024-01 | — | Full official NSRL RDS import / quarterly update UX | P0: optional local **SHA-256** hash-list only; RDSv2 MD5/SHA-1 unsupported; off by default | residual polish |
| D-0024-02 | — | Interactive filter builder / ad-hoc UI query | Presets + thin Run cull only in 0024 | **0028** |
| D-0024-03 | — | MD5/SHA-1 native digests for legacy DeNIST | Desk identity is SHA-256; fail closed on MD5-looking lists | residual if ever needed |
| D-0024-04 | P3 | Full GUI smoke for Run cull click path | Automated handler + unit tests; operator smoke local | operator / polish |
| D-0024-05 | P3 | Dedicated family-phase mid-write cancel integration test | Items-phase cancel/resume proven; family cancel covered in engine path | polish |
| (promote) | — | 0025 unique-only without cull | **Closed in 0025**: `auto` → `unique_only` when cull never run; `require_dedupe` optional fail | **0025** |

## From track 0025-PromoteToReview

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0025-01 | — | Multiple concurrent review sets / batch checkout | P0: one default `Review Corpus`; schema supports multi-set | later |
| D-0025-02 | — | Expand full email threads into review set | P0: **bidirectional family** only; threads → **0056** | **0056** |
| D-0025-03 | — | Interactive saved-search promote | Preset policies only | **0028** |
| D-0025-04 | P3 | Full GUI smoke for Promote to review click path | Automated handler + unit tests; operator smoke local | operator / polish |

## From track 0026-ReviewListViewer

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0026-01 | P3 | Large corpus (>50k `in_review`) loads first 500 rows only; no page/window nav | Documented threshold; API supports limit/offset; rare for MVP corpora (Codex F-006) | scale polish / **0028** filters |
| D-0026-02 | P3 | Full GUI smoke for Review list/keyboard/body path | Automated tempfile list+body + unit tests; operator smoke local | operator / polish |
| D-0026-03 | — | HTML browser engine / image render in body pane | P0: plain text + block-aware strip only | later |
| D-0026-04 | — | Multi review-set switcher in Review UI | Default set only; schema multi-set exists | D-0025-01 / later |
| D-0026-05 | — | Persist `last_review_item_id` across app sessions | In-session restore by id after list refresh only | optional polish |
| D-0025-01 | — | Multiple concurrent review sets | Unchanged; 0026 default set only | later |

## From track 0027-CodingAndBatch

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0027-01 | — | Privilege log export / 502(d) workflow | **Done in 0031** (`item_privilege` + withhold + privilege log CSV + protocol stub) | — |
| D-0027-02 | — | Filter list / saved search by code | **Done in 0028** (code any_of/none_of/missing + desk chips) | — |
| D-0027-03 | — | Auto-propagate to near-dup / full thread | Never default; family = parent+all children only | residual / **0056** |
| D-0027-04 | — | QC sampling reports / multi-reviewer lock | **Partial close in 0058**: sampling QC + item locks + force-unlock via matter service; Desk multi-reviewer UX residual | **D-0058-01** / residual |
| D-0027-05 | P3 | Full GUI smoke for coding panel / batch / digits path | Automated matter-core + desk unit/integration tests; operator smoke local | operator / polish |
| D-0027-06 | — | Production export of coded subsets | Membership only in 0027; **closed in 0040** (`scope=item_ids` / review corpus) | — |

## From track 0028-FiltersSavedSearch

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0028-01 | P3 | Keyset/cursor pagination if deep OFFSET still slow | P0: LIMIT/OFFSET + partial `idx_items_review_list_order`; Codex residual | residual scale |
| D-0028-02 | — | Nested saved-search-as-condition / deep OR builder | P0: flat AND only (Relativity nesting timeout risk) | residual |
| D-0028-03 | P3 | Full GUI smoke for filter bar / saved search / Load more | Automated matter-core + desk unit/integration; operator smoke local | operator / polish |
| D-0028-04 | — | Body keyword in FilterSpec | **Done in 0029** (Tantivy keyword box + compose; not FilterSpec SQL) | — |
| D-0026-01 | P3 | Large corpus paging | **Improved in 0028**: filtered count + Load more + compound list index; unfiltered Load more too | residual if multi-million |

## From track 0029-KeywordFtsSearch

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0029-01 | P3 | FTS hit window capped at 50k unique ids before filter intersect | Documented `DEFAULT_FTS_FETCH_LIMIT`; keyset/streaming deferred (Codex residual) | residual scale |
| D-0029-02 | — | CJK tokenizers (jieba/lindera) | **Closed in 0054**: hybrid CJK n-gram FTS (`cjk_ngram_v1`); dictionary jieba/lindera residual | **D-0054-01** |
| D-0029-03 | — | Fuzzy / proximity dtSearch parity | P0 Boolean + phrases only | residual |
| D-0029-04 | — | Snippet highlight UI | Optional SnippetGenerator / temporary FTS hit paint | residual / **0030** (nice-to-have; not DoD) |
| D-0029-05 | — | SQLite FTS5 primary | Forbidden by plan §4.7 | never |
| D-0029-06 | — | Crash left duplicate Tantivy docs | **Done in 0029**: delete-before-add + HashSet de-dupe | — |
| D-0029-07 | — | Windows mmap rebuild Access Denied | **Done in 0029**: drop readers + desk busy gate before rebuild | — |
| D-0029-08 | P3 | Full GUI smoke for keyword / Update / Rebuild | Automated matter-search + desk unit; operator smoke local | operator / polish |

## From track 0030-NotesHighlights

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0030-01 | — | Image/PDF box markups & burn-in redaction | **Text path closed in 0032** (regions + true redacted CAS). Full PDF/image geometric burn-in still deferred | **0034** |
| D-0030-02 | — | Notes in production load file | **Closed in 0040** (default exclude; residual opt-in) | residual opt-in |
| D-0030-03 | — | Privilege log narrative from notes | **Partial complete in 0031**: optional “draft from note” confirm only; never auto-export notes | — |
| D-0030-04 | — | Case-wide persistent keyword highlight sets | User highlights only; FTS paint optional | residual |
| D-0030-05 | — | Multi-user concurrent note edit | **Partial close in 0058**: service notes + OCC/locks/strict actor; Desk Connect residual | **D-0058-01** |
| D-0030-06 | — | Rich text / markdown notes | P0 plain text | residual |
| D-0030-07 | P3 | Full GUI smoke for notes panel / selection highlight | Automated unit + API; operator smoke local | operator / polish |
| D-0030-08 | P3 | Dual body widgets (Label paint + TextEdit selection) | Usable; document residual under egui 0.34; unify later if API allows | residual polish |
| D-0029-04 | — | Temporary FTS hit paint | Not shipped in 0030 (nice-to-have); user highlights shipped | residual |

## From track 0031-PrivilegeWorkflow

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0031-01 | — | Production enforce withhold fail-closed | **Closed in 0040** (skip + `fail_if_withheld`; late/TOCTOU recheck; purge artifacts) | — |
| D-0031-01b | — | Soft-clear description must not appear on produced load-file metadata | **Closed in 0040** (DAT field set has no privilege description columns) | — |
| D-0031-02 | — | Partial redaction produce + log “produced redacted” | **Partial complete in 0032**: `partial_redaction` + redacted text CAS + regenerate; packaging / “produced redacted” load-file still **0040** | **0040** |
| D-0031-03 | — | Category / thread-collapsed privilege logs | P0 standard document-by-document CSV only | residual |
| D-0031-04 | — | Name normalization legend for log parties | Metadata as stored | residual |
| D-0031-05 | — | AI privilege prediction / draft log descriptions | Off by default | Series G |
| D-0031-06 | — | Clawback post-produce workflow UI | Protocol notes only in 0031 | residual / **0040** |
| D-0031-07 | — | Multi-reviewer privilege lock / sampling QC | **Partial close in 0058**: privilege mutates + locks + sampling QC on service; Desk multi-reviewer residual | **D-0058-01** / **0041** residual |
| D-0031-08 | P3 | Full GUI smoke for privilege panel / log export | Automated API + unit; operator smoke local | operator / polish |
| D-0031-09 | — | Court e-file / load-file Bates on privilege log | ControlNumber = item_id until production | **0040** |
| D-0031-10 | — | Optional ParentFrom/ParentTo extra CSV columns | P0: in-place inherit into From/To/… is enough | residual |

## From track 0032-RedactionV1 (Completed — Codex luna PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0032-01 | — | Full PDF/image geometric redaction + content burn-in | P0 is text-body regions + redacted text CAS only | **0034** / residual |
| D-0032-02 | — | Native DOCX/XLSX redaction | Text path only | **0033**+ |
| D-0032-03 | — | Production packaging of redacted text + load file | **Closed in 0040** (`redacted_text_sha256` only when redactions; never original; synthetic EML uses redacted body) | — |
| D-0032-04 | — | QC fail produce if stale redactions / missing artifact | **Closed in 0041** (`redacted_text_missing` error + produce `require_qc_pass`) | — |
| D-0032-05 | — | AI suggested redaction ranges | Human-only P0 | Series G |
| D-0032-06 | — | Metadata header field redaction | Body display text only | residual |
| D-0032-07 | — | Inverse / full-page redaction tools | Relativity-style | residual |
| D-0032-08 | P3 | Full GUI smoke for redact mode / regenerate | Automated API + unit; operator smoke | operator / polish |
| D-0032-09 | — | Fixed-width blackout tokens matching span length | P0 fixed `[REDACTED]` token | residual |
| D-0032-10 | — | MuPDF / `redactor` crate PDF path | License + native deps review before core Desk | residual / **0034** |
| D-0032-11 | — | Redact-all-instances of same string in one document | P0 current selection only | residual |
| D-0032-12 | — | Metadata field redaction + body→metadata match suggestions | Everlaw-style; load-file field redact with **0040** | residual / **0040** |
| D-0032-13 | — | Stamp text inside produce blackout token | P0 fixed `[REDACTED]`; `label` is UI/list metadata | residual |
| D-0032-14 | P3 | Privilege hook not same-transaction as redaction create | Region commits then public upsert; rare partial state if hook fails; happy path tested | polish |

## From track 0033-OfficeExtractors (Completed — Codex luna PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0033-01 | — | Legacy OLE .doc/.xls/.ppt binary Office | P0 OOXML only; unsupported error | residual |
| D-0033-02 | — | Password-encrypted OOXML recovery | Honest `encrypted_office` error only | residual |
| D-0033-03 | — | Headers/footers/comments/track-changes full fidelity | Body/cells/slides best-effort P0 | residual |
| D-0033-04 | — | Embedded OLE / images OCR inside Office | Text path only | residual / **0036** |
| D-0033-05 | — | Native Office redaction (DOCX/XLSX) | Text redaction is 0032; natives untouched | residual (D-0032-02) |
| D-0033-06 | — | Full Office preview / WYSIWYG | Review shows extracted plain text | residual |
| D-0033-07 | — | LibreOffice convert sidecar | Forbidden P0 | residual |
| D-0033-08 | — | Auto-run office_extract after pst extract | **Partial 0043:** office_extract stage in built-in profiles / `profile_run` (not silent auto after every extract_pst) | residual / partial **0043** |
| D-0033-09 | P3 | Full GUI smoke for Extract Office text | Automated job + unit; operator smoke | operator / polish |
| D-0033-10 | — | Macro-enabled .docm/.xlsm execute | Never execute; text extract best-effort only | never |
| D-0033-11 | — | calamine still may allocate large range matrices internally | P0 mitigates with early text-cap break + native size cap; streaming sheet API if calamine adds one later | residual polish |

## From track 0034-PdfExtractPreview (Completed — Codex luna PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0034-01 | — | OCR for empty **and** low-text PDFs | **Consumed in 0036**: OCR success sets `pdf_needs_ocr=0` + review `text_sha256` | — |
| D-0034-02 | — | First-page / multi-page **raster preview** | **Locked deferred** — no pure-Rust full renderer in P0; future optional PDFium/MuPDF feature | residual |
| D-0034-03 | — | PDFium / MuPDF bundled native engine | Forbidden as required P0 dep | residual optional feature |
| D-0034-04 | — | Geometric PDF redaction burn-in | Not extract track | residual (D-0032-01) |
| D-0034-05 | — | Multi-page interactive PDF viewer | Residual with preview engine | residual |
| D-0034-06 | — | Password recovery / owner-password bypass | Encrypted → fail closed | never |
| D-0034-07 | — | Adversarial glyph/font extract hardening | Document best-effort extract ≠ visual | residual |
| D-0034-08 | — | PDF portfolio / embedded file tree | Single stream text P0 | residual |
| D-0034-09 | P3 | Full GUI smoke Extract PDF / needs-OCR chip | Automated job + unit; operator smoke | operator / polish |
| D-0034-10 | — | Auto-run pdf_extract after pst extract | **Partial 0043:** pdf_extract stage in built-in profiles / `profile_run` (not silent auto after every extract_pst) | residual / partial **0043** |
| D-0034-11 | — | Tunable MIN_TEXT_CHARS thresholds per matter | P0 fixed constants (50 total / 20 per page) | residual |

## From track 0035-CalendarItems (Completed — Codex luna PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0035-01 | — | Full PidLid named-property map (Location, Busy, Recurrence blob, …) | P0 standard tags + ICS; honest nulls | residual |
| D-0035-02 | — | RRULE expansion to all occurrences | P0 flag + text only; no infinite expand | residual |
| D-0035-03 | — | Exception instances / series graph | Residual | residual |
| D-0035-04 | — | Month/week calendar UI | Review text + list only | residual |
| D-0035-05 | — | Tasks / contacts (`IPM.Task`, …) | Calendar classes only | residual |
| D-0035-06 | — | Live Graph/Exchange calendar APIs | Export/ICS/PST only | never |
| D-0035-07 | P3 | Full GUI smoke calendar chip / ICS job | Automated + operator smoke | operator / polish |
| D-0035-08 | — | Dedicated FilterSpec `cal_start_at` field | P0 maps start→sent_at when email times null | residual polish |
| D-0035-09 | — | Calendar-specific logical_hash preimage polish | non-email hash / UID path P0 | residual |
| D-0035-10 | — | Produce archive-parent multi-event ICS explicitly | **Closed in 0040** (selected child native only; parent only if selected) | — |
| D-0035-11 | — | Floating times / exotic non-IANA TZIDs | Fail-soft null offset; no invent | residual |
| D-0035-12 | P3 | Embedded VTIMEZONE not used for offset resolution | IANA chrono-tz only; blobs copied into child natives | residual |
| D-0035-13 | P3 | Force multi-child rewrite via `update_item` leaves FTS bookkeeping until reindex | ICS apply path clears FTS; update_item does not | residual polish |

## From track 0036-OcrPlugin (Completed — Codex luna PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0036-01 | — | Bundle Tesseract/Poppler in Windows installer | P0: operator installs; path in Settings | residual packaging |
| D-0036-02 | — | In-process leptess / tesseract-rs linking | P0 CLI sidecar only | residual |
| D-0036-03 | — | Cloud OCR providers | Never default offline product | never / Series G |
| D-0036-04 | — | Auto-run OCR after pdf_extract | **Closed in 0043:** OCR stage in `with_ocr` built-in + user profiles / `profile_run` | — |
| D-0036-05 | — | Multi-language pack UI | P0 `lang` string (default eng) | residual |
| D-0036-06 | — | Handwriting / layout/table OCR | Plain text Tesseract | residual |
| D-0036-07 | — | Write OCR text layer back into PDF native | Text CAS only | residual |
| D-0036-08 | — | OCR after redaction burn-in pipeline | P0 skip when redaction_count>0 | residual / **0040** |
| D-0036-09 | P3 | Live Tesseract+osd rotated-scan smoke | Mock path automated; operator + real Tesseract | operator / polish |
| D-0036-10 | P3 | Full GUI smoke enable OCR + Run | Automated job + unit; operator smoke | operator / polish |
| D-0036-11 | — | Soft per-page timeout (e.g. 120s) | Cancel between pages/items; soft timeout residual | residual polish |
| D-0036-12 | — | Encrypted matter-scoped temp for page bitmaps | Drop + purge P0; full temp encryption residual | residual |
| D-0036-13 | P3 | Mid-doc checkpoint resume at exact next page + partial text | Cancel pauses without apply; resume retries item from page 1 (safe) | residual polish |

## From track 0037-FileCategoryTaxonomy (Completed)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0037-01 | — | Fine-grained `file_type` subtype (e.g. docx vs category document) | P0 is closed `file_category` only | residual |
| D-0037-02 | — | Full Nuix/Relativity 900–1000+ MIME catalog parity | Never claim; workstation-like families only | never / residual |
| D-0037-03 | — | Chat / Teams deep type signals | **Closed in 0055**: category `chat` + `teams_extract` HTML/PST/JSON adapters + conversation_id; deeper live Teams type signals residual | residual / D-0055-* |
| D-0037-04 | — | Mobile / cloud package type packs | Thin reserved categories | residual |
| D-0037-05 | — | User-editable custom taxonomy UI | Closed vocabulary P0 | residual |
| D-0037-06 | — | AI content-based classification | Offline metadata only | Series G |
| D-0037-07 | — | Auto-run classify in processing profiles | **Closed in 0043:** classify stage in `standard` / `extract_only` + `profile_run` | — |
| D-0037-08 | — | Load-file / QC % unrecognized gates | Taxonomy enables fields | **0040** / **0041** |
| D-0037-09 | — | Deep CFB CLSID sniff to distinguish .msg vs legacy Office without extension | P0: extension disambiguation after OLE magic (§3.4.1) | residual |
| D-0037-10 | — | Full ZIP central-directory OOXML detection for renamed containers without office extension | P0: peek when possible + extension tie-break; bare zip → archive | residual polish |
| (0024) | — | File category taxonomy expansion for cull | **Closed in 0037** (`taxonomy_v1` + classify job + noise_light executable) | — |

## From track 0038-CaseOverviewDashboard

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0038-01 | — | Exportable CSV/PDF progress & matter reports | **Closed in 0039** (`export_matter_report` CSV pack; PDF → D-0039-01) | — |
| D-0038-02 | — | Materialized overview snapshot table for multi-million items | P0 live GROUP BY + indexes + top-N + concurrent fan-out | residual scale |
| D-0038-03 | — | Click-through from category/custodian/error-code row → FilterSpec | Tables first | residual / **0028** |
| D-0038-04 | — | egui_plot bar charts for top types | Optional polish; tables satisfy DoD; pin match eframe 0.34 | residual |
| D-0038-05 | — | Multi-matter portfolio dashboard | Single matter P0 | residual / **0058** |
| D-0038-06 | — | Continuous auto-refresh / live per-second charts | Manual + post-job refresh P0 | residual |
| D-0038-07 | — | Gap analysis (missing mailbox/date vs opposing) | Not overview | **0042** |
| D-0038-08 | — | People/comms timeline heatmaps | **Partial close in 0047**: day/week timeline **tables** + Top Pairs/people; force-graph/heatmap charts residual | residual / D-0047-05 |
| D-0038-09 | P3 | Full GUI smoke Overview panel | Automated API + unit; operator smoke local | operator / polish |
| D-0038-10 | — | Physical source package size (path stat / sources.size column) | P0 top-level item size only | residual |
| D-0038-11 | — | “Reviewed” beyond codes (opened, notes-only, privilege-only) | P0: ≥1 item_code | residual |
| D-0038-12 | — | Dedicated connection pool crate (r2d2/sqlx) for overview | P0: multi open_for_read + threads | residual |
| D-0038-13 | — | Error rollup by stage (in addition to code) | P0 top-N by code | residual |

## From track 0039-ProgressReporting (Completed — Codex luna PASS)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0039-01 | — | Pure-Rust PDF summary of matter report | CSV shipped; if later: **embedded TTF** required (§3.5.1) | residual |
| D-0039-02 | — | Full per-row item_errors detail CSV | Size risk; scrub paths if ever shipped | residual |
| D-0039-03 | — | CLI `report export` headless | **Closed in 0045** (`report export`) | — |
| D-0039-04 | — | UTF-8 BOM for Excel | Dual datetime is P0; BOM polish if needed | residual |
| D-0039-05 | — | Scheduled / email delivery of reports | Never default | residual / SaaS |
| D-0039-06 | — | Embed report in production package | Optional attach | residual |
| D-0039-07 | — | Multi-matter portfolio report | Single matter P0 | residual / **0058** |
| D-0039-08 | P3 | Full GUI smoke Export matter report | Automated API + unit; operator smoke | operator / polish |
| D-0039-09 | — | Job engine never stores raw client paths in `error_summary` | Report scrub is P0; source hygiene residual | residual / process-runner polish |
| D-0039-10 | — | Expand finite `STABLE_CODES` allowlist as new job codes ship | Privacy-first; unknown snake_case redacted | residual polish |
| D-0038-01 | — | Exportable reports handoff | **Closed in 0039** | — |

## From track 0040-ProductionExport (Completed — Codex luna PASS)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0040-01 | — | TIFF/PDF image productions + OPT/LFP | No image factory P0 | residual / image redaction |
| D-0040-02 | — | CLI `produce` headless | **Closed in 0045** (`produce run` / `job run --kind produce`) | — |
| D-0040-03 | — | Broken-family QC (orphan attach / missing parent) | **Closed in 0041** (orphan error; incomplete_parent any missing non-withheld child warn) | — |
| D-0040-04 | — | Privilege log co-export into volume `PRIVILEGE/` | Separate 0031 export remains | residual |
| D-0040-05 | — | Matter report attach into volume `REPORTS/` | Soft residual (D-0039-06) | residual |
| D-0040-06 | — | CP1252 single-byte Concordance DAT mode | P0 UTF-8 + BOM | residual |
| D-0040-07 | — | Space-collapse multi-line field mode | P0 uses Concordance `®` | residual |
| D-0040-08 | — | Notes opt-in load-file columns | Default exclude | residual |
| D-0040-09 | P3 | Full GUI smoke Produce dialog / job path | Automated API + unit; operator smoke | operator / polish |
| D-0040-10 | — | Slip sheets / placeholders for withheld | Skip only P0 | residual |
| D-0031-01 | — | Withhold fail-closed packaging | **Closed in 0040** | — |
| D-0032-03 | — | Redacted text packaging | **Closed in 0040** | — |

## From track 0041-ProductionQcRules (Completed — PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0041-01 | — | Post-volume path/file existence QC | P0 pre-produce selection rules | residual |
| D-0041-02 | — | TEXT folder privilege-term search | Human/residual | residual |
| D-0041-03 | — | Auto-fix (auto expand family, auto regenerate redacted, auto slip-sheet) | Report-only P0 | residual |
| D-0041-04 | — | Custom user-defined QC SQL rules | Builtin pack P0 | residual |
| D-0041-05 | — | Multi-jurisdiction QC packs | **Partial close in 0060**: named packs `qc_default_v1` / `qc_strict_privilege_v1` / `qc_native_heavy_v1` bound by production profiles; fingerprint includes pack id | residual firm packs |
| D-0041-06 | — | Sampling / multi-reviewer QC UI | **Partial close in 0058**: API sampling QC + JSON report; Desk QC UI residual | residual / **D-0058-01** |
| D-0041-07 | — | CLI `qc run` | **Closed in 0045** (`qc run`) | — |
| D-0041-08 | — | Full findings table in SQLite | CSV + qc_runs + fingerprint enough P0 | residual |
| D-0041-09 | P3 | Full GUI smoke Run QC / produce block / stale | Automated engine + unit; operator smoke | operator / polish |
| D-0041-10 | — | QC max-age TTL in addition to fingerprint | Fingerprint is hard P0 invariant | residual |
| D-0041-11 | — | Raise incomplete_parent default to error | P0 warn (protocol-dependent) | residual |
| D-0041-12 | P3 | Jump-to-Review when item not in loaded list | Falls back to first row; filter/not-in-review residual | residual / polish |
| D-0041-13 | P3 | Soft-gate continuous re-poll while Produce open | Mitigated by start_produce recheck + job terminal + hard gate | residual / polish |
| D-0040-03 | — | Broken-family QC handoff | **Closed in 0041** | — |
| D-0032-04 | — | Stale redaction produce QC | **Closed in 0041** | — |

## From track 0042-GapAnalysis (Completed — PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0042-01 | — | Fuzzy custodian name / alias matching beyond case-fold | P0 exact normalize + **warn** severity | residual |
| D-0042-02 | — | Ingest opposing natives into CAS | Metadata set-diff only P0 | residual |
| D-0042-03 | — | Foreign DAT auto column-detect ML | Enum map + 0040 default map P0 | residual |
| D-0042-04 | — | Day-level date holes / heatmap UI | **Forbidden P0**; week/month only | residual |
| D-0042-05 | — | Purview legal-hold / hold-notice roster sync | Manual expected list P0 | residual |
| D-0042-06 | — | OPT/image opposing productions | DAT metadata P0 | residual |
| D-0042-07 | — | CLI `gap run` | **Closed in 0045** (`gap run`) | — |
| D-0042-08 | P3 | Full GUI smoke Gap panel | Automated + operator smoke | operator / polish |
| D-0042-09 | — | Emit `MESSAGE_ID` on 0040 produce DAT for foreign-style re-import | Self-compare uses ITEM_ID/CONTROL P0 | residual / produce polish |
| D-0042-10 | — | Raise missing_custodian default to error after alias table ships | P0 locked **warn** | residual |
| (mid-index) | — | Per-row MID full scan O(n·m) | **Closed in 0042**: `message_id_index` + bulk compare | — |

## From track 0043-ProcessingProfiles (Completed — PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0043-01 | — | `parent_job_id` column for UI grouping of profile child jobs | **Closed in 0044**: `jobs.parent_job_id` + index; profile_run + workflow_run children set parent; desk Parent column | — |
| D-0043-02 | P3 | Neardup skip-already when `reset:false` (still re-sketches) | Off in built-ins; documented residual | residual |
| D-0043-03 | P3 | Full form profile editor | Save-as + clone from built-in/user is P0 | residual polish |
| D-0043-04 | P3 | Desk progress stage flicker during `profile_run` (shared progress sink) | **Improved in 0044**: poller preserves handler stage/message for `profile_run`/`workflow_run` (count-only); residual polish | residual polish |
| D-0043-05 | P3 | Full GUI smoke profile dropdown / Apply / Run profile | Automated engine + unit; operator smoke | operator / polish |
| D-0036-04 | — | Auto-run OCR after pdf_extract | **Closed in 0043** | — |
| D-0037-07 | — | Auto-run classify in profiles | **Closed in 0043** | — |

## From track 0044-WorkflowEngine (Completed — Codex luna PASS)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0044-01 | — | Extract-all-PSTs fan-out under a source | P0 single `pst_item_id` binding | residual |
| D-0044-02 | — | True parallel multi-handler stages per matter | Sequential multi-job only; SQLite single-writer | residual |
| D-0044-03 | — | Firm-wide **user** workflow template pack | Built-ins app-global; user matter-local; multi-user pack later | residual (not closed by 0058) |
| D-0044-04 | — | Visual workflow editor / DAG designer | Built-ins + API upsert; no graph UI | residual |
| D-0044-05 | — | Branch / alt-path nodes on prior failure | Ordinary soft_fail without full graph | residual |
| D-0044-06 | P3 | Full GUI smoke workflow dropdown / Run / parent jobs | Automated engine + unit; operator smoke | operator / polish |
| D-0044-07 | P3 | Desk CRUD for user workflows (JSON editor) | Select/run P0; upsert via API | residual polish |
| D-0019-01 | — | Multi-job parallel stages | **Partial closed in 0044** (sequential); true parallel → D-0044-02 | residual |
| D-0043-01 | — | parent_job_id | **Closed in 0044** | — |

## From track 0045-CliAutomationParity (Completed — see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0045-01 | — | Fire-and-forget / `--no-wait` detach | P0 always waits for terminal | residual |
| D-0045-02 | — | Cross-process cancel of in-flight job | `job cancel` marks DB; SIGINT cancels in-process runner. **0078 makes in-process cancel observable** (exit 130) but does **not** close this — cross-process cancel is still unimplemented. | residual |
| D-0045-03 | — | Binary rename to `dedupe-cli` | Keep `pst-dedup` P0 | residual |
| D-0045-04 | P3 | Schema-driven path tags beyond known key list | Known keys preflight P0 | residual polish |
| D-0019-02 | — | Full CLI job control | **Closed in 0045** | — |
| D-0016-09 | — | CLI ingest | **Closed in 0045** | — |
| D-0018-05 | — | CLI extract | **Closed in 0045** via `job run --kind extract_pst` | — |
| D-0039-03 | — | CLI report export | **Closed in 0045** | — |
| D-0040-02 | — | CLI produce | **Closed in 0045** | — |
| D-0041-07 | — | CLI qc | **Closed in 0045** | — |
| D-0042-07 | — | CLI gap | **Closed in 0045** | — |

## From track 0046-EntityPiiPacks (Completed — see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0046-01 | — | NER / ML entity extractors | P0 offline regex packs only | **0051+** |
| D-0046-02 | — | User-authored regex packs (JSON) | Built-ins only P0; if later, still `regex` crate only | residual |
| D-0046-03 | — | Auto-add `entity_scan` to processing profiles / workflows | Manual job / CLI only P0 | residual |
| D-0046-04 | — | Auto-redact / create redaction from entity hit | Operator uses **0032** manually | residual |
| D-0046-05 | — | HTML body (`html_sha256`) scan | Prefer plain `text_sha256` P0 | residual |
| D-0046-06 | — | Cross-item same `match_hash` report UI | Hash index enables later; graph does not surface match_hash report; thin API residual | residual |

## From track 0047-PeopleCommsGraph (Completed — see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0047-01 | — | Force-directed / canvas people graph viz | P0 tables-first only | residual polish |
| D-0047-02 | — | Manual alias merge UI (display↔smtp) | No auto-merge P0 | residual / **0051** |
| D-0047-03 | — | Entity-body emails in graph (`include_entity_emails`) | P0 rejects `true` fail-closed; headers only | residual (policy) |
| D-0047-04 | — | Multi-hop path UI (recursive CTE) | Residual | residual |
| D-0047-05 | — | Heatmap charts for timeline / pairs | Tables in 0047; charts residual | residual / D-0038-08 |
| D-0047-06 | — | Incremental dirty Pass 1 | P0 full rebuild; Pass 2 always from participants | scale residual |
| D-0047-07 | — | BCC-in-pairs export/UI toggle | Default Top Pairs = visible only (to+cc); no BCC column | residual advanced |
| D-0047-08 | P3 | Fingerprint inventory digest for soft-stale | Fingerprint = engine+params; desk defaults `reset:true` | residual polish |
| D-0047-09 | P3 | SQLite UNIQUE + NULL person_id on timeline | Pass2 delete+rebuild; sentinel residual | residual polish |
| D-0047-10 | P3 | Auto-reload People panel when job completes | Manual Refresh P0 | residual polish |
| D-0017-03 | — | Relational `item_participants` | **Closed in 0047** | — |


## From track 0048-ClusteringConceptMining (Completed - see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0048-01 | — | Embedding / BERTopic-style pipeline | **Partial:** **0050** shipped opt-in semantic *search* (mock embeddings + chunk index). BERTopic-style *clustering* redesign still residual | residual / **0050** search done |
| D-0048-02 | — | Hierarchical / HDBSCAN soft clusters | Residual | residual |
| D-0048-03 | — | Multi-set UI + compare sets | Schema multi-set; Desk default set only | residual |
| D-0048-04 | — | Cluster bubble / treemap viz | Tables-first P0 | residual polish |
| D-0048-05 | — | Multilingual stopwords / CJK tokenizers | **Partial 0054**: CJK FTS n-gram; multi-lang stopword lists residual | **D-0054-03** |
| D-0048-06 | — | Mid-iteration empty-centroid reseed | Final empty drop always applied | residual |
| D-0048-07 | — | Deeper reply-quote strip for clustering | Shares D-0023-01 residual | residual |
| D-0048-08 | — | Exclude near-dup members by default | Residual param; off by default | residual |
| D-0048-09 | — | Incremental re-cluster dirty docs | P0 full rebuild | scale residual |
| D-0048-10 | — | LLM cluster titles | Opt-in only | **0051/0052** |

## From track 0049-SentimentNlpPlugin (Completed — see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0049-01 | — | Transformer / ONNX sentiment | P0 lexicon/rules only; **0050** is semantic *search* (not sentiment transformers). Real MiniLM/Candle residual under **D-0050-01** / **0051** | residual / **0051** |
| D-0049-02 | — | Multilingual lexicons | English P0; residual after 0054 FTS packs | **D-0054-04** |
| D-0049-03 | — | Per-unit score table + highlight UI | Aggregation is unit-based; no per-unit persist | residual |
| D-0049-04 | — | Aspect-based (entity targets) | Residual | residual |
| D-0049-05 | — | Emotion taxonomy beyond pos/neu/neg | Residual | residual |
| D-0049-06 | — | Dashboard tone heatmaps | Residual polish | residual |
| D-0049-07 | — | Auto-suggest codes from polarity | **Never default** | never default |
| D-0049-08 | — | Job scope `in_review` | P0 `all` only | residual |
| D-0049-09 | — | Subject prepend as first unit | Optional residual | residual |
| D-0049-10 | P3 | Wire remaining fixtures via `include_str!` | pos/neg used; hostile still partly inline | residual polish |
| D-0049-11 | P3 | Formal `cargo deny` in CI for sentiment tree | Manual tree audit in review.md | residual |

## From track 0050-SemanticSearchPlugin (Completed — see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0050-01 | — | Full Candle / MiniLM production load | P0 MockEmbedder + fail-closed `semantic-candle` stub; no weights in git | residual / operator |
| D-0050-02 | — | Cloud / remote embeddings | Forbidden P0 | **0051** |
| D-0050-03 | — | Hybrid FTS ∩ semantic rank fusion | Keyword and semantic remain separate paths | residual |
| D-0050-04 | — | Cross-encoder re-ranker | Residual | residual |
| D-0050-05 | — | HNSW (or ANN) at multi-million scale | P0 exact cosine + pre-filter | residual |
| D-0050-06 | — | Multilingual embed models | English-centric mock/P0; residual after 0054 FTS packs | **D-0054-05** |
| D-0050-07 | — | GPU acceleration path | CPU-only mock path | residual |
| D-0050-08 | — | Multi-model UI + namespace GC | Namespaces exist; one active model; no GC UI | residual |
| D-0050-09 | — | RAG chat + citations | Citation-rich promote closed in **0052**; multi-turn/cross-doc RAG residual | residual / D-0052-01 |
| D-0050-10 | — | Embedding-based clustering | Residual vs **0048** | residual |
| D-0050-11 | — | Packed `vectors.bin` format | P0 JSON per-item files under namespace | residual polish |
| D-0050-12 | P3 | Formal `cargo deny` in CI for semantic tree | Manual `cargo tree` audit in review.md | residual polish |
| D-0050-02 | — | Cloud / remote embeddings | Still residual; 0051 provides chat-shaped provider trait (not embedding API). Embed path may reuse later | residual / **0051** closed channel only |

## From track 0051-AiProviderTrait (Completed — see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0051-01 | — | Streaming completions | Unary complete only | residual |
| D-0051-02 | — | Multi-turn chat / RAG UI | P0 thin suggest only; multi-turn still residual | **D-0052-01** |
| D-0051-03 | — | Citation-rich promote UX | **Closed in 0052** (citations + verify + mandatory highlight/scroll + pointer audit) | — |
| D-0051-04 | — | Cloud embeddings via AiProvider trait | Chat completions shape only; see D-0050-02 | residual |
| D-0051-05 | — | Auto privilege / redaction AI | Human confirm only; never silent | residual |
| D-0051-06 | — | Prompt-injection hardening suite | Residual | residual |
| D-0051-07 | — | Azure-specific auth variants | Base URL + key covers many; residual | residual |
| D-0051-08 | — | Empty model-result fingerprint marker | Empty `[]` leaves no suggestion row; may re-call provider | residual polish |
| D-0051-09 | P3 | Live HTTP redirect-to-remote CI proof | Fail-closed `Policy::none()` + 3xx error path unit-tested; no mock server in CI | residual polish |
| D-0051-10 | P3 | Formal `cargo deny` in CI for matter-ai tree | Manual tree audit in review.md | residual polish |

## From track 0052-AiReviewCitations (Completed — see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0052-01 | — | Multi-turn chat / cross-doc RAG | P0 is single-item grounded citations only | residual |
| D-0052-02 | — | Batch accept-all with sampling QC | P0 single-item promote; 0058 sampling QC is human review QC not AI batch accept | residual |
| D-0052-03 | — | Persistent multi-citation highlight sets | P0 is click-to-highlight active citation | residual polish |
| D-0052-04 | — | Export AI provenance report pack | Still no cleartext quotes unless redacted export policy | residual |
| D-0052-05 | — | Privilege / redaction AI with citations | Human-confirm residual | residual |
| D-0052-06 | — | Semantic chunk inject into prompt | **0050** residual | residual |
| D-0052-07 | — | `ai_enrich_citations` split job | Same-call v2 citations sufficient for P0 | residual |
| D-0052-08 | P3 | Bodies larger than verify continuous cap (2 MiB) | Continuous prefix only; head+tail never used for offsets | residual scale |
| D-0052-09 | P3 | Full egui smoke for citation scroll/paint click path | Unit helpers + API tests; operator smoke local | operator / polish |
| D-0052-10 | P3 | `VERIFY_OFFSET_MISMATCH` stored status unused | Reserved; runtime repairs to matched or quote_not_found | residual polish |

## From track 0053-TranscriptionPlugin (Completed — see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0053-01 | — | Cloud STT APIs | Residual + allow_remote if ever | residual |
| D-0053-02 | — | Speaker diarization | Un-diarized honesty is P0; human must listen for attribution | residual |
| D-0053-03 | — | Timed segment table / SRT export | Residual | residual |
| D-0053-04 | — | Auto-enqueue `fts_index` after transcribe | P0 documents manual rebuild | residual |
| D-0053-05 | — | GPU-only acceleration path | Residual | residual |
| D-0053-06 | — | Multilingual model packs UI | Residual after 0054 FTS packs | **D-0054-06** |
| D-0053-07 | — | In-app media player | Residual | residual |
| D-0053-08 | P3 | Upgrade symphonia 0.5 → ~0.6 | P0 uses 0.5.x for stable SampleBuffer/Probe API | residual polish |
| D-0053-09 | P3 | Pre-convert duration probe for non-WAV | Post-ffmpeg WAV duration enforced; pre-convert residual | residual polish |
| D-0053-10 | P3 | Live whisper.cpp + ffmpeg operator CI smoke | Mock + Job Object kill tests in default CI; no weights in git | operator / polish |

## From track 0054-MultilingualPacks (Completed — see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0054-01 | — | jieba / lindera dictionary tokenizers | P0 is hybrid CJK n-gram (`cjk_hybrid_v1`); dictionary residual if tantivy 0.26 pin allows | residual |
| D-0054-02 | — | Machine translation plugin | Explicit out of scope; never default cloud | residual / never default |
| D-0054-03 | — | Full multi-lang cluster stopwords | English list in 0048; zh/ja/ko thin sets residual | residual |
| D-0054-04 | — | Multilingual sentiment lexicons | English VADER P0 | residual (was D-0049-02) |
| D-0054-05 | — | Multilingual embed models | English-centric semantic P0 | residual (was D-0050-06) |
| D-0054-06 | — | STT multi-model language UI | Whisper model path only in 0053 | residual (was D-0053-06) |
| D-0054-07 | — | Per-item pack routing | Matter-level pack P0 only | residual |
| D-0054-08 | — | OCR tessdata pack manager | Residual 0036 path docs | residual |
| D-0054-09 | — | Batch `lang_detect` job | Thin API + `set_item_language_tag` shipped; full job residual | residual |
| D-0054-10 | P3 | Bare `+tag@example.com` QueryParser operator | Index preserves plus-address; quote in query (`"+tag@…"`) | residual polish |

## From track 0055-TeamsChatAdapters (Completed — Codex luna PASS)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0055-01 | — | Live Teams Graph Export API collection | Explicit out of scope; offline packages only | residual / never default |
| D-0055-02 | — | Physical SharePoint attachment hydrate | P0 injects `[Attachment:]` / URL lines only | residual |
| D-0055-03 | — | Full RSMF file export format | Day-bucket conversation_id is P0; full RSMF residual | residual |
| D-0055-04 | — | Hour-level or custom bucket grain | Fixed 24h UTC day P0 | residual |
| D-0055-05 | — | Edit/delete version timeline UI | Not in P0 | residual / **D-0056-06** |
| D-0055-06 | — | Conversation review chrome | **Closed in 0056** (Conversations screen + day-bucket stream) | — |
| D-0055-07 | — | Meeting recording auto-STT chain | Use 0053 STT on media residual | residual / 0053 |
| D-0055-08 | — | Private channel mailbox discovery | Collection residual; adapter sees package only | residual |
| D-0055-09 | — | `teams_extract` in processing profiles | Explicit Desk/CLI job P0; profile stage residual | residual / profile polish |
| D-0055-10 | P3 | Real Purview HTML variance beyond fixture parser | Versioned `html_fixture_v1` only | residual polish |
| D-0055-11 | P3 | Full GUI smoke for Run Teams extract + Chat chip | Automated job + unit tests; operator smoke local | operator / polish |

## From track 0056-ConversationReviewUi (Completed — Codex luna PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0056-01 | P3 | Notes / privilege panels on Conversations tools pane | Coding + body only P0; Linear Review remains for notes/privilege | residual polish |
| D-0056-02 | P3 | Full FTS/filter hit set beyond loaded Review pages on handoff | Handoff always includes target + loaded thin rows; unpaged ≤50k passes full set via rows | residual scale |
| D-0056-03 | — | Email `thread_id` conversation mode | Chat/`conversation_id` only P0 | residual / D-0022 |
| D-0056-04 | — | Multi-conversation bulk code | Single day-bucket bulk is P0 | residual |
| D-0056-05 | — | Nested reply trees (vs inline chrome) | Inline “In reply to” P0 | residual |
| D-0056-06 | — | Edit/delete version timeline UI | From D-0055-05 | residual |
| D-0056-07 | — | Infinite scroll auto-load | Keyset Load earlier / Load more P0 | residual polish |
| D-0056-08 | — | Conversation transcript export HTML | Residual | residual |
| D-0056-09 | P3 | Full GUI smoke Conversations list/stream/handoff/bulk | Automated API + unit; operator smoke local | operator / polish |
| D-0025-02 | — | Expand full email threads into review set | Still residual (0056 did not ship email thread mode) | residual |
| D-0022-01 | — | Full JWZ dual objects as review items | Still residual | residual |
| D-0027-03 | — | Auto-propagate codes to full thread | Never default; day-bucket bulk is explicit opt-in only | residual |

## From track 0058-MultiUserMatterService (Completed — Codex luna PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0058-01 | — | Desk “Connect to service” UX (URL + login + session actor) | **Closed in 0064** — Home Connect dialog, Connected banner, thin remote review | **closed** |
| D-0058-02 | — | TLS / mutual TLS for LAN bind | Loopback default; `--allow-lan` without TLS P0 | residual |
| D-0058-03 | — | Lock heartbeat / renewal | TTL-only P0 (default 4h) | residual |
| D-0058-04 | P3 | True dual-process exclusive-lock CI stress | Real `fs4` exclusive lock; same-process test may soft-pass | residual polish |
| D-0058-05 | P3 | Concurrent read path under service (`open_for_read` pool) | WriteGate serializes all handlers P0 | residual scale |
| D-0058-06 | — | `PST_DEDUPE_SERVICE_TOKEN` long-lived automation token | Password login + bearer sessions P0 | residual |
| D-0058-07 | P3 | Redaction privilege hook OCC under multi-user | Service P0 mutates codes/notes/privilege; redaction path residual | residual polish |
| D-0058-08 | — | Multi-matter host process | One matter per process P0 | residual |
| D-0058-09 | — | Fine-grained field-level RBAC | Three roles P0 | residual |
| D-0038-05 | — | Multi-matter portfolio dashboard | Still residual | residual |
| D-0039-07 | — | Multi-matter portfolio report | Still residual | residual |

## From track 0059-MultiTenantSso (Completed — Codex luna PASS)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0059-01 | — | SAML 2.0 IdP | OIDC Auth Code + PKCE is P0 | residual |
| D-0059-02 | — | Desk browser “Sign in with SSO” UX | **Advanced in 0064** — loopback handoff + `POST /v1/oidc/exchange`; clipboard paste still not production DoD | residual polish (custom URI scheme, cross-process handoff) |
| D-0059-03 | — | IdP RP-initiated / back-channel logout | Local logout + lock release is P0 | residual |
| D-0059-04 | — | Multi-matter single process host | Still one matter per process; D-0058-08 | residual |
| D-0059-05 | — | Per-tenant matter CMK / external KMS | Distinct from platform IdP secret PMK; `TenantKeyProvider` stub only | **D-0057-03** |
| D-0059-06 | — | SCIM user provisioning | Residual | residual |
| D-0059-07 | — | Postgres / multi-region platform.db | SQLite platform.db is P0 | residual scale |
| D-0059-08 | P3 | Configurable public base URL for OIDC redirect (TLS proxy) | P0 derives `http://{bind}` | residual polish |
| D-0059-09 | P3 | OIDC discovery metadata cache TTL | In-process cache; process restart refreshes | residual polish |

## From track 0060-MultiJurisdictionProduction (Completed — Codex luna PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0060-01 | — | CP1252 / legacy encoding DAT path | UTF-8 BOM default; fail-closed CP1252 residual (D-0040-06) | residual |
| D-0060-02 | — | Desk produce profile dropdown | **Closed in 0064** — Solo produce dialog profile picker + required Bates start + pre-flight | **closed** |
| D-0060-03 | — | Auto suggest next Bates (MAX prefix) | Start still explicit required | residual |
| D-0060-04 | — | Image + OPT/LFP production profiles | D-0040-01; name_by_bates extends to images | residual |
| D-0060-05 | — | Full Relativity load-file suite | Alias map only P0 | residual |
| D-0060-06 | — | Firm-wide profile pack sync | Matter-local upsert is enough P0 | residual |
| D-0060-07 | — | UK/EU/AU full protocol packs | Beyond template tags / jurisdiction_tag | residual |
| D-0060-08 | P3 | Volume README.txt hardcodes DATA/NATIVES/TEXT + UTC wording | Profile layout/date may differ; DAT is authoritative | residual polish |
| D-0041-05 | — | Multi-jurisdiction QC packs | **Partial close in 0060** (named packs + profile binding) | residual firm packs |

## From track 0064-DeskPlatformConnectUx (implementation — closes D-0058-01 / D-0060-02)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0064-01 | — | Full Connected feature parity (notes/privilege/locks/batches/FTS/AI/jobs) | P0 thin list/body/codes only | residual matrix |
| D-0064-02 | — | Remote produce / job start via HTTP | Service has no produce routes; Solo/host CLI | residual / future track |
| D-0064-03 | — | Persist bearer in OS keyring across restarts | Process-memory session only for P0 | residual polish |
| D-0064-04 | — | Custom URI scheme handoff (`dedupe-desk://`) | Loopback handoff is P0; scheme residual | residual polish |
| D-0064-05 | — | Dev-only clipboard bearer paste | **Banned** as production SSO path | residual / never for DoD |
| D-0064-06 | — | Handoff code durable across service multi-instance | In-memory handoff codes; single host P0 | residual |
| D-0064-07 | P3 | Desk↔service in-process integration test (login + codes 409) | Unit/helpers cover builders + state; router round-trip residual (Codex) | residual polish |
| D-0064-08 | P3 | Abortable body transport (async reqwest cancel) | Single-flight blocking worker + latest-wins meets DoD-7 fallback; true abort residual (Codex) | residual polish |

## From track 0061-CloudBlobJobBackends (Completed — Codex luna PASS)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0061-01 | — | Full remote worker fleet / K8s via HTTP JobBackend client | Physics locked (HTTP to matter-service only; never remote SQL); LocalProcessRunner P0 | residual |
| D-0061-02 | — | Azure Blob backend open path | Feature flag residual; trait ready; P0 is S3-compatible | residual |
| D-0061-03 | — | GCS object backend | residual | residual |
| D-0061-04 | — | Live dual-write / migration local→S3 | Single active backend P0; migrate tooling residual | residual |
| D-0061-05 | — | Hosted SQLite / network matter.db | Explicitly out of P0 (never NFS) | residual / never |
| D-0061-06 | — | OpenSearch SearchBackend | residual Series later | residual |
| D-0061-07 | — | External per-tenant CMK for object store | Overlaps D-0057-03 | residual |
| D-0061-08 | P3 | Cache re-hash on every hit | Size/path consistency P0 | residual polish |
| D-0061-09 | P3 | Multipart upload tuning at TB scale | 10 MiB × 2 concurrent is P0 ceiling | residual polish |
| D-0061-10 | — | Desk UI settings panel for storage backend | Headless + CLI P0; admin UI residual (dangerous) | residual polish |
| D-0061-11 | P3 | Live mid-handle rebind after `storage set` without reopen | Config persisted; CAS activation on next open | residual polish |
| D-0061-12 | P3 | Encrypted remote `blob_len` streams object for AEAD header | Correct; optional HEAD/metadata residual | residual polish |

## From track 0057-SecurityHardener (Completed — see conductor review)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0057-01 | — | Convert existing plaintext matter → encrypted | Full re-encrypt residual; create-encrypted only P0 | residual |
| D-0057-02 | — | Encrypted zip transfer package for counsel | Plan open item | residual |
| D-0057-03 | — | FIPS / enterprise CMK | Series I multi-tenant | residual (D-0059-05) |
| D-0057-04 | — | Biometric unlock | Residual | residual |
| D-0057-05 | — | Secure wipe free space after seal | Unlink-only wipe of `.enc-db` | residual polish |
| D-0057-06 | — | FTS mmap-class perf on encrypted matters | P0 honesty accepts no-mmap EncryptedDirectory | residual research |
| D-0057-07 | — | Encrypt `semantic/` vector store under DEK | P0 encrypts DB+CAS+FTS only | residual |
| D-0057-08 | — | Stream-encrypt CAS put without any plaintext staging file | Staging now under `workspace/temp/.cas-stage` and purged; zero-staging residual | residual polish |
| D-0057-09 | P3 | Desk seeds `PST_DEDUPE_MATTER_PASSPHRASE` in process env for worker opens | Prefer in-memory DEK share later; clear env on lock residual | residual polish |
| D-0057-12 | P3 | Drop cannot return seal errors; not all paths call `seal_encrypted()` | CLI change-passphrase seals; Drop retries seal and keeps session live on fail | residual polish |
| D-0057-10 | P3 | Full GUI smoke encrypt create / unlock / change passphrase | Automated API + unit; operator smoke local | operator / polish |
| D-0057-11 | — | SQLCipher page encryption path | Pure-Rust AEAD file container is P0 equivalent; SQLCipher needs OpenSSL/perl | residual optional feature |
| D-0036-12 | — | Encrypted matter-scoped temp for page bitmaps | Matter workspace/temp is boundary when encryption_enabled; deeper page-bitmap residual | residual |

## From track 0065-ScanIntegrityReport (residuals)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0065-orphan-walk | — | NBT orphan discovery (messages without resolvable folder path) | P0 ships `is_orphaned` always `false` from folder walker; field + reasons ready | residual |
| D-0065-resume | — | Multi-GB mid-folder checkpoint/resume | Full re-walk every run; multi-file continues under `--allow-failed-files` | residual |
| D-0065-ansi | — | ANSI PST support | Maps to `ANSI_UNSUPPORTED`; no read path | residual |
| D-0065-soft-body | P3 | Soft partial body byte recovery (keep truncated text) | P0 sets `body_incomplete` on truncation/CRC body Err without partial bytes; materialize honors unavailable flags | residual |
| D-0065-class-b | — | Soft structural Class B rebuild of broken folder tables | Out of scope; Class C repair forever out | residual / never |

## From track 0066-DedupKeepSetExport (residuals)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0066-disk-groups | — | Disk-backed group/candidate store for multi-million message runs | P0 holds O(n) candidate metadata in RAM; streaming outputs only | residual / scale |
| D-0066-materialize-dir | — | Optional `--materialize-dir` smoke export of winner bodies | **Closed by 0067** — product surface is `pst-dedup unique-eml --out` (volume-batched EML pack) | **closed / 0067** |
| D-0066-soft-body | P3 | Soft partial body bytes on materialize when extract returns incomplete | P0 surfaces `BODY_UNAVAILABLE` / scan degraded flags; no partial-byte recovery | residual (D-0065-soft-body) |
| D-0066-fine-fidelity | — | Multi-level fidelity rank (soft reasons vs body/attach loss) | **Closed in 0075** (`--fidelity-rank graded`; binary remains default) | **closed / 0075** |

## From track 0067-UniqueEmlPackCli (residuals)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0067-gui-keepset | — | **Soft-closed in 0072:** GUI primary unique export is keep-set **unique-pst** wizard; legacy EML retained as secondary (“Export Unique EML (legacy scan path)”) | Full EML co-export residual: **D-0071-also-eml** | **0072** |
| D-0067-embedded-depth | P1 | Full recursive nested MAPI extract for embedded messages | **Narrowed in 0094:** unique-pst method-5 nested `WriteMessage` export + `PidTagAttachDataObject` PtypObject + winner-only extract + child stream via `open_attach_data_from_message_node`. **0101 shipped unique-pst CLI:** `--max-embedded-depth` operator-tunable **1–8** (default **3**; clap rejects outside that range). **Residual stays:** unique-eml nested MIME `message/rfc822` packaging; matter/Relativity child-document extract; 32 MiB per-nest; hard cap 8. **Do not close** this row. | residual (unique-eml MIME / matter children) |
| D-0067-long-path | — | Windows `\\?\` long-path support when abs root already > budget | P0 truncates subject to keep abs ≤250; extreme deep roots may still fail that file | residual |
| D-0067-cloud-attaches | — | Resolve hyperlink-only / cloud modern attachments | Not downloaded; no invented file bytes | residual |

## From track 0068-ProductionPstWriterV1

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0068-01 | — | No subnode-diversion path for oversized non-body string props (subject/sender/display_to/message_class) | **Closed in 0093:** `push_string_prop` + cumulative escalate+reprobe on MessageSize probe; `message_class` on the helper; `MAX_HEAP_VALUE_SIZE` = 2048 documented as single-page HeapBuilder deviation (not MS-PST 3580). | **closed / 0093** |
| D-0068-02 | — | `scanpst.exe` / Outlook-open structural verification (§3.9 DoD-11) | **Closed in 0080** for the automatable half: `scanpst` runner (local temp copy, `-no repair` verified-or-skip, log-parse, timeout+kill, `.bak` hard error) + source-differential Tier A QC + `qc_attestation_v1` for the human half. Operator-local scanpst on multi-GB remains optional evidence (D-0079-operator-multigb / D-0080-scanpst-arg). | **closed / 0080** |
| D-0068-03 | — | CLI `pst-dedup write-pst` subcommand | **Closed in 0071** via product `unique-pst` (keep-set → streaming write → report pack). Thin standalone `write-pst` not required. | **closed / 0071** |
| D-0068-04 | — | Recipient table, attach table schema, full named-prop set | **Attach table closed in 0069** (NBT template 0x671 + per-message TC + RowIndex). **Recipient half closed in 0082** (template `0x692` + 14 MUST columns + optional SmtpAddress; empty TC always present; BCC opt-in). **Named-prop read allowlist closed in 0084** (NPMAP parse + PSETID_Attachment / `AttachmentProviderType` resolve for cloud detect). Residual: full named-prop encyclopedia + full NPMAP **write** (see **D-0084-cloud-named-prop-write**) | **closed / 0082** (recipient); **narrowed / 0084** (reader allowlist); residual full set / write |
| D-0068-05 | — | `PidTagIpmWastebasketEntryId`/`PidTagFinderEntryId` + Deleted Items/Search folder objects | **Done in 0068** (round 9): re-raised twice by cross-model review (rounds 5/6) and declined both times on the reasoning that creating Deleted Items/Search folder objects was track-0069 folder-tree work. Round 9 re-raised it a third time with specific Microsoft Learn URLs; the orchestrator fetched and read those pages directly and confirmed `PidTagIpmWastebasketEntryId`/`PidTagFinderEntryId` are 2 of the 5 MS-PST "Minimum Set of Required Properties" for a message store PC (not richness), and that IPM_SUBTREE's own required-initialization page documents a "Deleted Items" hierarchy-TC row — reversing the decline. Deleted Items (real folder, empty) and Search Root (`NID_TYPE_SEARCH_FOLDER`, real folder, empty) are now written, with the store carrying both EntryIDs. Whether Outlook actually requires these to **open** is still explicitly **unverified** here (no scanpst/Outlook available — same residual as D-0068-02); `docs/pst-writer-fidelity-v1.md` states this plainly. | operator scanpst gate (D-0068-02) resolves definitively |
| D-0068-06 | — | MS-PST fixed "template object" tables (NID `0x60D`–`0x610`) + IPM_SUBTREE `PidTagDisplayName`/`PidTagContentCount`/`PidTagContentUnreadCount`/`PidTagSubfolders` | **Done in 0068** (round 9): previously declined (round 6) as an Outlook-internal creation-time UI optimization not needed to open/traverse an existing file. Round 9 re-verified directly against the four individual MS-PST template-object specification pages, each of which states its table "MUST have no data rows" as a structural file requirement — implemented as four fixed, always-empty TCs with correct column schemas. Also fixed a literal-string bug where IPM_SUBTREE's `PidTagDisplayName` was written as `"IPM_SUBTREE"` instead of the MS-PST-required `"Top of Personal Folders"`, and added the previously-declined `PidTagContentUnreadCount`/`PidTagSubfolders` (now verified as part of the same required-initialization source page, not orchestrator judgment). See `docs/pst-writer-fidelity-v1.md`. | operator scanpst gate (D-0068-02) resolves definitively |
| D-0068-07 | P3 | `PtypMultipleInteger32` (0x6805, FAI template column `PidTagOfflineAddressBookTruncatedProperties`) modeled as a 4-byte HNID-reference column width, not independently verified against an authoritative MS-PST multi-value-property byte-width spec | Explicitly flagged by the implementer (round 9) as a judgment call, not a verified fact — the FAI contents table template (NID `0x60F`) is permanently zero-row per MS-PST's own "MUST have no data rows" requirement, so this width choice never affects actual stored/read data, only the always-unpopulated column-descriptor bookkeeping for that one template object. Internal review independently confirmed the choice is at least internally consistent (no TC layout overlap/collision) but did not independently verify the real MAPI multi-value byte width. | future polish / resolve if a later track ever needs a genuinely populated multi-value TC column |

## From track 0069-PstWriterFidelity (Completed — Codex luna PASS)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0069-stream-buffer | — | One-attach full `Vec` buffer via `AttachStreamSource` (no multi-GB zero-hold stream) | **Closed in 0070**: `open_attach_stream` + `Layout::write_data_chain_from_reader` (chunk = `MAX_BLOCK_DATA`) | **closed / 0070** |
| D-0069-embed-object | P2 | `PidTagAttachDataObject` (PtypObject) on embedded attaches | **Closed in 0094:** `PcValue::Object` writes PtypObject `0x000D` on `0x3701` (8-byte heap `{Nid, ulSize}`); reader resolves via property first, subnode-scan fallback for 0069-era files. | **closed / 0094** |
| D-0069-casefold | P3 | Folder/prefix case key uses ASCII `to_uppercase`, not full Unicode casefold | Sufficient for Windows-oriented eDiscovery paths; residual if exotic Unicode folder names appear | residual polish |
| D-0068-02 | — | scanpst / Outlook structural verification | **Closed in 0080** (see 0068 table) | **closed / 0080** |
| D-0067-cloud | — | Cloud/ref attach download | Skip + fail count in 0069 | residual |
| D-0068-04 attach | — | Attachment table schema | **Closed in 0069** | — |

## From track 0070-PstWriterStreamingScale

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0070-eager-spill | — | Eager write / spill of all leaf block payloads so `Layout` never holds multi-GB of leaf `Vec`s | **Closed**: `EagerWriteCtx` + `Layout::push_leaf_block` place+write leaves to same-dir temp (`on_disk=true`, empty `data`). Residual in RAM: small XBLOCK/XXBLOCK/SLBLOCK/PC heaps only | **closed / 0070 P1** |
| D-0070-dto-collect | — | Full `WriteMessage` DTO list collected for multi-source folder planning | **Closed in 0070**: streaming path uses `IncrementalFolderPlan` (one-pass consume; no DTO pre-collect). Caller-owned `Vec` RAM is the caller's; fat in-memory bodies on DTOs remain the caller's responsibility | **closed / 0070** |
| D-0070-multi-source-stream-prefix | — | Multi-source folder prefixes on streaming path use sources **seen so far** | **Closed in 0095**: `WritePstOpts.known_source_paths` pre-seeds `IncrementalFolderPlan` so prefixes are stable from message 1 when ≥2 distinct sources are known. unique-pst CLI passes distinct winner `source_path`s. Also: consecutive leading IPM/root alias strip; lazy Unique Mail in preserve; QC Deleted Items claimable + `normalize_folder_path_key` expected/out alignment. | **closed / 0095** |
| D-0070-operator-multigb | — | Operator/nightly multi-GB synthetic stress + optional scanpst | CI capped (~16 MiB attach stream); full multi-GB not committed | operator residual |
| D-0070-inline-hash-io | P3 | Final hash is a full sequential read of the temp after seeks (not byte-at-a-time inline writer) | Correct vs final bytes; second sequential I/O on multi-GB. **Narrowed by 0079 §2.4/§3.7:** inline hashing is *impossible* as the writer is built, not merely undone — finalize seeks back to rewrite header, AMap pages, NBT and BBT (`production.rs:1524-1540`), so the final bytes do not exist until after those seeks. 0079 owns only the buffer size + sequential-read hint. The residual shrinks to "restructure finalize to write strictly forward" — a writer-format track. | residual / writer-format |
| D-0068-02 | — | scanpst / Outlook structural verification | **Closed in 0080** (see 0068 table) | **closed / 0080** |
| D-0069-stream-buffer | — | Chunked attach stream | **Closed in 0070** | — |

## From track 0071-CliUniquePstAndReport

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0071-also-eml | — | `--also-eml` co-export unique-eml pack alongside unique-pst | Flag accepted; co-export not wired in P0 (operators can run `unique-eml` separately) | residual / optional |
| D-0071-operator-outlook | — | Operator Outlook / scanpst open of unique-pst multi-volume output | **Closed in 0080** — same family as D-0068-02; scanpst + attestation + per-volume first/last sampling. | **closed / 0080** |
| D-0071-dest-nid | P3 | Optional `dest_nid` column on export_messages.csv | Writer does not surface destination NIDs to CLI today | residual polish |

## From track 0072-DeskUniquePstWizard

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| (soft-close) | — | D-0067-gui-keepset | Soft-closed: Unique PST wizard is primary GUI unique export; legacy EML secondary | **0072** |
| D-0072-operator-gui-smoke | P3 | Full interactive egui click-path smoke for wizard | Unit tests cover args/cancel/log/repaint/preflight; operator interactive residual (same class as other Desk tracks) | operator / polish |

## From track 0073-ExportAttachmentFailureLedger

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0073-promote | P1 | Mode A pre-write promote-on-attach-fail | **Closed in 0083**: `--promote-on-attach-fail` (default off); three `decided_by` strings; Mode C fallback; `winner_promoted` wired. **Mode B write-time promote permanently declined.** | **closed / 0083** |
| D-0073-eml | P2 | unique-eml attach skip ledger parity | **Closed in 0089**: `EmlAttachEvent` on `EmlWriteResult`; CLI `AttachLedgerSink` at `{out}/export_attachments.csv` (same `EXPORT_ATTACHMENTS_CSV_HEADER`); Mode A soft-skip + `mark_promoted_winner`; `--attach-ledger` / `--attach-ledger-max-rows` / `--ledger-path-mode`; fail-closed ledger init. Counters remain classify source of truth. | **closed / 0089** |
| D-0073-gui | P3 | GUI wizard attach-ledger mode / summary UI | CLI flags default full; GUI uses defaults via UniquePstCliArgs | residual polish |
| D-0073-basename | P3 | `--ledger-path-mode=full\|basename` handoff redaction | **Closed in 0081**: flag default `full`; applies to `export_messages.csv` + `export_attachments.csv` path columns only; `source_id` join key; Matter Archive mapping mandated in runbook | **closed / 0081** |
| D-0073-vec-events | P3 | Writer still accumulates `attachment_fidelity_events` Vec | **Closed in 0077**: first-N cap 1000 + `attachment_fidelity_events_truncated` / `_total`. **0079 declined the channel-only redesign with reason** (§2.4): converting a bounded 1000-element `Vec` to a channel adds a thread and a failure mode to save at most a few hundred KiB. Recorded so it is not re-raised as a free win. | **closed / 0077; declined 0079** |

## From track 0074-DeepAttachPreflightFidelity

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0074-gui | P3 | GUI wizard checkbox / summary for deep-attach preflight | CLI `--deep-attach-preflight` works; wizard defaults off via UniquePstCliArgs | residual polish |
| D-0074-mat-lru | P3 | Bound materializer/export sticky PST handle map to `max_open_psts` LRU | **Closed in 0079**: one shared bounded LRU `PstHandleCache` (`--max-open-psts` default 32) via `Rc<RefCell<…>>` for materializer + attach stream. | **closed / 0079** |
| D-0074-cache-share | P3 | Cross-process or durable scan→unique probe result cache | In-process level/mtime/size cache only for phase 1b→materialize | residual polish |
| D-0074-crc-fixture | P3 | Synthetic corrupt-attach PST E2E for CRC/open/read | **Closed in 0077**: generate-at-test-time via `pst-writer` + byte flips (`crc_integrity_0077` tests); never real-file derived. | **closed / 0077** |
| D-0074-timeout-join | P3 | Join/cancel timed-out per-attach probe worker | `recv_timeout` returns ATTACH_PROBE_TIMEOUT; worker may finish in background; budget charged | residual polish |
| D-0074-e2e-fixture | P3 | Full production-path §3.11 fixture matrix (scan JSON/CSV + unique-pst) | **Closed in 0080** — QC fixture matrix + negative corrupt-output tests (`unique_pst_qc_0080`) are the production-path E2E matrix. | **closed / 0080** |

## From track 0075-KeepSetWinnerPolicies

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| (soft-close) | — | D-0075-scope custodial / vertical dedupe | **Closed in 0076** via `--dedupe-scope per-source` | **closed / 0076** |

## From track 0076-ContentHashTierHardening

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0076-default-v2 | — | Switch default identity to v2 strong hash | Would change every stored `content_hash_hex` | product |
| D-0076-attach-content | P2 | `--strong-content-hash body-recip-attach` full-stream digests + Choice B unread | **Closed in 0086:** live levels `off\|body\|body-recip\|body-recip-attach`; streaming SHA-256; name+size domain-separated unread sentinels; dedicated budgets; soft warn with ignore-inline | **closed / 0086** |
| D-0076-recipient-table | P2 | Recipient table (SMTP + PidTagRecipientType) instead of display strings | **Closed in 0082:** Tier-2.5 uses per-row identity cascade SMTP → EX DN → display when table present; table-less messages keep display path; To+Cc+Bcc in identity | **closed / 0082** |
| D-0076-inline-attach | P3 | Residual edge cases for inline detection | **Shipped** MAPI flags on attach PC (`0x3712` / `attRenderedInBody` / hidden) + `inline_attachments_ignored`; residual only for stores that omit those tags | residual |
| D-0076-bulk-class | — | Template / Newsletter class for large MID-distinct clusters | Stats surface cluster; classification is product | residual |
| D-0076-custodian-map | — | Many PSTs → one custodian map | Needs operator-supplied mapping surface | residual |
| D-0076-gui | P3 | Desk surfaces for scope / identity enums | Checkbox for body only (consistent with D-0075-gui) | residual polish |
| D-0076-operator-perf | — | Multi-GB performance proof for default / body levels | Fixture-scale: aspose scan ~51 ms default / ~66 ms `--strong-content-hash body` (debug); multi-GB residual | operator-local |
| D-0076-fixture-baseline | P3 | Checked-in fixture-wide pre-0076 grouping baseline matrix | Synthetic refinement + ASPOSE winner golden green; full aspose/promotions group baselines residual | residual polish |
| D-0076-normalize-parity | — | Full body normalization parity with Relativity (strip spaces) | Changing v1 normalization is a hash change | residual |
| D-0075-gui | P3 | Desk free-text `--folder-rank` / `--source-rank` lists | Wizard has earliest_date + Prefer folder class + Prefer BCC; ordered lists CLI-only | residual polish |
| D-0075-storeids | — | Store-EntryID special-folder detection (`PidTagIpmWastebasketEntryId`, …) | Keyword ladder + `--folder-rank` sufficient for P0 | residual |
| D-0075-locale | — | Localized folder-name packs (zh/de/fr/ja) | Segment globs are workaround only | residual |
| (soft-close) | — | D-0066-fine-fidelity | **Closed in 0075** via opt-in `--fidelity-rank graded` | **closed / 0075** |

## From track 0077-CrcNoiseAndExportRisk

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0077-tracing-layer | P3 | Optional `tracing` rate-limit Layer for third-party consumers of `pst-reader` | Primary mechanism is data-path counters (Desk has no subscriber); Layer is residual | residual |
| D-0077-parallel-attrib | P3 | Per-source CRC attribution under parallel materialize (0079) | Sequential snapshot/delta is exact today; comment at scan snapshot names this residual. Sequential path correct; residual is parallel materialize only. **0079 §3.8 specifies the answer** — under `--jobs > 1`, emit `crc_attribution: "aggregate"` and **omit** per-source CRC fields rather than filling them with a plausible guess. Closed only if `--jobs` actually ships; 0079 may decline to ship it if §3.3–3.6 already hit the target. | residual / **0079** |
| D-0077-desk-subscriber | P3 | Install a tracing subscriber in release Desk builds | Counters already reach `summary.json` without a subscriber; log lines remain CLI-centric | residual polish |
| D-0077-gui | P3 | Desk per-source CRC counter tables / distinct-bad-BID drill-down | Banner + export_risk stats row shipped; richer UI residual | residual polish |
| D-0077-repair-diff | P3 | `pst-dedup compare-counts` wrapper for ScanPST before/after | **Closed in 0081 (docs):** two-command `scan --json` before/after on ScanPST **copy**; no `compare-counts` CLI | **closed / 0081** |
| D-0077-systematic-poly | P3 | Unique-pst `export_risk` ignores dual-rate poly class (raw `block_crc_read_rate` / `attach_stream_crc_events` still refuse) | **Closed in 0099:** thresholds key on **effective** (non-poly) rate; poly-only jobs (INC*) do not auto-`not_export_ready`. Dual-rate (`page≥0.50` AND `block≥0.50`) remains the classifier. Raw CRC telemetry retained. High block alone keeps taint **and** blocks Tier-2. Residual fingerprint: `D-0077-poly-fingerprint`. Residual job-level attach CRC: `D-0099-attach-crc-job-level`. | **closed / 0099** |
| D-0077-poly-fingerprint | P3 | True CRC polynomial / Permute allowlist vs dual-rate heuristic | Split from D-0077-systematic-poly. Dual-rate stays the 0077/0099 classifier. Fingerprint is a later reader track if dual-rate ever mis-classifies localized corruption as poly. Streaming unique may under-merge until keep-set rebuild (0077 residual). | residual |

## From track 0099-CrcPolyExportRiskHonesty (Completed)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0099-attach-crc-job-level | P3 | Write-time `ATTACH_STREAM_CRC` is a job-level sum; poly discount keys off **scan-time** `crc_noisy` / `poly_class_crc` | Spec §3.7 declined per-event writer attribution (enough for INC* all-poly). Residual: a poly+clean job could discount attach CRC that actually came from write-time reads of the clean source, because scan may not have streamed those attach blocks. Do not treat as a 0099 regression. | residual / after **0099** |
| D-0099-oracle-inputs-attest | P3 | `export_oracle` attest pointers `/export_risk/inputs/…` never compare | PR #89 Bugbot: `"inputs"` was on `SUMMARY_ALLOWLIST_KEYS`, so `strip_keys_recursive` deleted `export_risk.inputs` before `compare_integrity_counters`. **Closed in 0102:** removed `"inputs"` from the allowlist; root `/inputs` blanking kept. | **closed / 0102** |

## From track 0078-UniqueExportExitCodes (planned)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0078-retryable | P3 | Transient (retry-safe) vs permanent failure — as a **`retryable: bool` JSON field, not a new exit code** | **Closed in 0082:** `summary.retryable` on unique-export JSON; true only for cancel / clear transient IO; permanent classes stay false. **No new exit integers.** Runbook still forbids blanket retry of exit 5. | **closed / 0082** |
| D-0078-gui | P3 | Desk surfacing of `fidelity` / `exit_reason` | 0077 banner already covers `export_risk` (the safety-critical half); wizard shows completion, not exit class | residual polish |

## From track 0079-MaterializeWritePerformance (Completed — Codex luna PASS WITH DEFERRED P3)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0079-deterministic-key | — | Derive the store record key from a digest of inputs so unique-pst output is byte-reproducible | **Closed in 0087:** domain-separated SHA-256 preimage (length-prefixed fields); path-independent; default deterministic; job-global `store_key_material` from ordered winner loci; ProviderUID == RecordKey. Full volume-file `sha256_hex` remains best-effort (B-tree/layout); structural oracle is the DoD-3 fallback. | **closed / 0087** |
| D-0079-reader-buffer | P3 | `PstFile` holds a single 64 KiB `BufReader` (`pst-reader/src/lib.rs:105`) that random block reads defeat | A seek discards the buffer, so serving a ~8 KiB block can cost a 64 KiB refill. Suspected read amplification on every materialize/probe/verify path. Belongs to a `pst-reader` track with its own fixtures — 0079 measures it, does not fix it. | residual / pst-reader |
| D-0079-stream-prepare | — | Pipeline prepare→write so RAM is bounded by in-flight winners rather than winner count | `prepared: Vec<PreparedWinner>` holds every winner's full `WriteMessage` incl. bodies before a single byte is written. Residual after 0079 shipped measurement + threshold warning; structural pipeline is Phase C / once `--jobs` plumbing exists. | residual / Phase C |
| D-0079-operator-multigb | — | Operator-local multi-GB before/after using the 0079 equivalence oracle + `PhaseTimings` | Carries D-0070-operator-multigb and satisfies D-0076-operator-perf. Cannot be CI (no real PSTs in git). Fixture residual ~300 ms wall; INC 275 s operator evidence remains the multi-GB gate for shipping `--jobs`. | operator |
| D-0079-seq-scan | P3 | Windows `FILE_FLAG_SEQUENTIAL_SCAN` on post-write hash open | std `File::open` cannot set the flag; 0079 declined a CreateFile re-open path and removed a fake no-op claim. Concurrent SHA-256+MD5 over 1 MiB buffer shipped instead. | residual polish |
| D-0079-cancel-latency | P3 | Numeric cancel→quarantine latency before/after | 0078 behavioral gate (exit 130 + quarantine + summary) retained green; fixture-scale wall numbers not instrumented as a dedicated cancel timer. | residual / polish |

## From track 0080-UniquePstOutlookQc (implemented — review pending)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0080-com-declined | — | Outlook COM automation (`--outlook-com-smoke`) | **Declined with reasons** (spec §3.9) — recorded so it is not re-raised as a free win. (a) New Outlook has **no COM/VSTO/VBA object model and none is planned**, and it has been the default client since April 2026; classic Outlook retires Q1–Q2 2028, EOL Q2 2029 — building `windows`-crate COM automation now buys a path that is already off by default. (b) Adding a store to an Outlook profile **mutates the operator's live client** on a machine holding real client mail; QC must be side-effect-free there. (c) `scanpst -no repair` validates the file *format* against Microsoft's own validator with no `unsafe`, no FFI and no new dependency — a strictly better signal for a fraction of the machinery, and COM's panic paths would have to be reconciled with the no-`unwrap`/no-`expect` production rule. Revisit only if Microsoft ships an automation surface for new Outlook | declined / revisit on MS change |
| D-0080-recipient-table | P2 | Real recipient table (SMTP + `PidTagRecipientType`) in written PSTs | **Closed in 0082:** writer emits template `0x692` + per-message recipient TC; `fidelity_contract_v1.recipient_table` → `Preserved`; QC source↔output compares written recipient structure (BCC filter respected) | **closed / 0082** |
| D-0080-bcc-policy | — | Whether unique-pst should ever write `PidTagDisplayBcc` | **Decided + shipped in 0082:** default OFF (To+Cc only); opt-in `--include-bcc-recipients`; identity still includes BCC when table present; `bcc_suppressed` ledger column + `bcc_suppressed_message_count` | **decided / 0082** |
| D-0080-scanpst-arg | P2 | Exact `-no repair` token + `-silent` behaviour confirmed against real Outlook builds | Microsoft's page prints `-no repair` (with a space). **Asymmetric failure:** unrecognized args fall into the **legacy repairing** path. **Production real `scanpst.exe` always Skips** unless build ≥ 16.0.10325.20082 **and** `PST_DEDUP_SCANPST_OPERATOR_VERIFIED=1` (operator attestation — help text alone is not behavioural proof). CI stubs (`.cmd`/`.bat`): Ok only when build pinned **and** log has success markers + `NO_REPAIR_MODE` — never bare `.accepts-no-repair` / `PST_DEDUP_SCANPST_NO_REPAIR_OK`. | operator residual |
| D-0080-external-reader-matrix | P3 | Which libpff (`pffinfo`) / libpst (`readpst`) versions were actually validated against our output | Sidecar is counts-only and skip-safe; licences are **LGPL-3.0-or-later** / GPL, so invocation is by **process only** — never bundled, linked, vendored, or added as a Cargo dependency. BYOB path only (rule 15) | operator / residual |
| D-0080-newoutlook | — | No durable automation successor once classic Outlook retires | **Updated 0081 (2026-07-29):** New Outlook **can open/add** `.pst` today (Settings → Files → Add file; classic side-by-side, same bitness) — stale “import, not mount” wording retired. Still **no COM** and Microsoft has stated limited future `.PST` investment once bulk import ships. PST remains a correct *deliverable* (Purview emits it; eDiscovery consumes it) — durable proof is source-differential reader QC + optional scanpst-on-copy, not client automation. Revisit when bulk import ships / classic EOL | product / watch |
| D-0080-cloud-attachments | P2 | Cloud/modern (OneDrive/SharePoint link) attachment-table detect | **Closed in 0084 (attach-table only):** NPMAP reader + `AttachmentProviderType` resolve; independent OR method/URL signals; `ATTACH_CLOUD_LINK` ledger + `cloud_provider`/`cloud_url` columns; Mode A incomplete; pointer-row preserve on unique-PST (no invented binary; no network hydration). Body-inline closed in **0085** (separate surface). Payload never Preserved offline | **closed / 0084** (attach-table detect) |

## From track 0084-NamedPropCloudAttach

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0084-body-cloud-links | P2 | Body-only / inline paste SharePoint/OneDrive URLs without MAPI attachment-table rows | **Closed in 0085:** offline document-shaped body URL scan (`export_body_cloud_links.csv` + `body_cloud_link_count`); commercial allowlist + caps; SafeLinks unwrap; no invent attach; no Mode A incomplete from body hits; Mode A physical-vs-inline known gap documented | **closed / 0085** |
| D-0084-cloud-named-prop-write | P3 | Full allowlist named-prop re-emit on unique-PST (NPMAP write + PSETID_Attachment props on attach PC) | **Closed in 0092:** allowlisted NPMAP write (streams + hash buckets, BucketCount=251) + MUST `PidNameAttachmentProviderType` / MAY Url when present; emit-only-when-used; classic LongPathname+Pathname retained. Residual: full named-prop encyclopedia / arbitrary source NPMAP clone still out of scope | **closed / 0092** |
| D-0092-permission-type-extract | P3 | Source extract of `AttachmentPermissionType` into canonical/`WriteAttachment` | **Closed in 0096:** reader `NAME_ATTACHMENT_PERMISSION_TYPE` + `AttachmentInfo.cloud_permission_type`; canonical/materializer/`write_attachment_from_canonical*`; QC live-read `AttachDetail` + fidelity Preserved; MAY-if-present open-world i32; write cloud-pointer only; hasher isolation. INC0102784 had 0 attach-table cloud providers | **closed / 0096** |

## From track 0085-BodyCloudLinks

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0085-sovereign-cloud-hosts | P3 | GCC High / DoD / other sovereign-cloud SharePoint + SafeLinks hostname suffixes | **Closed in 0088:** body-cloud allowlist extended with `*.sharepoint.us`, `admin.onedrive.us`, `*.sharepoint-mil.us`, `*.dps.mil`, and SafeLinks `*.safelinks.protection.office365.us`. 21Vianet (`*.sharepoint.cn`) excluded; GCC Moderate uses commercial endpoints (no extra hosts). SafeLinks unwrap mainly historical (SP/OD no longer wrapped per Learn 2026-05-22) | **closed / 0088** |
| D-0088-usgovcloud-microsoft-tld | P3 | GCC High `.microsoft` TLD content hosts (`*.usgovcloud.microsoft`, `*-usercontent.microsoft`, …) | Learn GCC High ID 23 (2026-07-01). **Out of 0088 P0** — do not guess document-shaped paths; record so it is not a silent miss later | residual / after **0088** |
| D-0080-unexplained-byte-edit | P3 | `unexplained_loss` is allowlist fail-closed for properties not in `fidelity_contract_v1`; PST byte-edits of preserved fields produce `defect`, not `unexplained_loss`. DoD-9 defect class proven by truncate/flip/CC strip; `unexplained_loss` proven by production record path + `extra_source_props` (unmapped property observation channel). | **By design (keep `extra_source_props`):** allowlist fail-closed for property names absent from `fidelity_contract_v1`. Production path: `record_classified_finding` + digest `extra_source_props` (QC product sidecar, not a probe). Truncated/corrupt PST ⇒ `defect`. Inventing an uncontracted MAPI prop via pure byte-edit is not a realistic writer failure mode. | residual / design |

## From track 0086-AttachContentIdentity

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0086-embedded-email-hash | P3 | Recursive Relativity-style four-component hash for `ATTACH_EMBEDDED_MSG` | **Closed in 0090:** `embedded-msg-hash/v1` for method-5 subnode + rfc822 (header+body+recip+child attaches; depth/byte/count budgets; **not Relativity parity**). Full nested **export** remains **D-0067-embedded-depth** | **closed / 0090** |
| D-0086-digest-probe-unify | P3 | Unify 0074 Full (L3) probe + identity digest into one streaming pass | **Closed in 0091:** record-don't-tee — Pass-1 Real digest seeds Full/ok probe cache; Pass-2 skips second stream I/O while charging probe tallies once (`digest_stream_skips`) | **closed / 0091** |

## From Series N (INC0102784 operator deep-dive, 2026-08-25)

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0097-body-cloud-truncate-honesty | P3 | Body-cloud CSV emits many `BODY_CLOUD_LINK_TRUNCATED` rows with empty `cloud_url` | **Closed in 0097:** C+A hybrid — `truncated` := dropped document-shaped candidates; 0 CSV rows for window-only 0-candidate; ≤1 marker/message (`BODY_CLOUD_LINK_WINDOW` / `BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED` / `BODY_CLOUD_LINK_URL_TRUNCATED`); split `body_scan_window_capped_messages`; over-length prefix is not a kept hit. Umbrella `BODY_CLOUD_LINK_TRUNCATED` removed. INC0102784 62 empty rows were window-hit, not lost links. | **closed / 0097** |
| D-0097-window-edge-normalize | P3 | Window-edge bare URL dedupe skips `normalize_candidate` | PR #88 Bugbot on Completed 0097: `handle_window_edge_bare` checks `acc.seen` on unclassified/raw URL. False extra `BODY_CLOUD_LINK_WINDOW` possible. Not Series P. | residual polish |
| D-0093-recipient-tc-multipage | P3 | Recipient TC beyond single-page HN (Strategy B budget cap) | **Closed in 0100** (PR #90, `ab1c7b0`): Strategy A row-matrix subnode + RowsPerBlock (`Floor(8176/56)=146`) + multi-block HN on the recipient-table node; shared `TableContext::load`. Production does not emit `RECIPIENT_TC_TRUNCATED`. Residual: HNBITMAPHDR (attach-table closed in **0104**; SLBLOCK NID order closed in **0103**). | **closed / 0100** |
| D-0093-attachment-tc-page | P3 | Attachment-table TC (NID `0x671`) is single-page and uncapped | **Closed in 0104:** Strategy A (row-matrix subnode + RowsPerBlock `Floor(8176/25)=327` + multi-page HN on the attachment-table node; cell NID for filenames &gt;2048 UTF-16 bytes). Production does not emit `ATTACH_TC_TRUNCATED`. Residual: HNBITMAPHDR (`D-0100-hn-bitmap-hdr`). | **closed / 0104** |
| D-0094-inc-resmoke | P3 | Operator re-smoke INC0102784 unique-pst after nested export | **2026-08-26 post-0098:** 4055/4055 verify; folder_tree_match true; exit 64 `ATTACH_SOFT_FAIL` only (4 depth-limit). `export_risk` poly-class lie **closed in 0099**. Recipient truncation **closed in 0100**. Depth CLI **shipped in 0101**. **HITL skipped** (no operator INC* `--max-embedded-depth 8` smoke this pass). Optional: `output/inc0102784-post-0101/` (operator-local; not CI). | residual / operator HITL |
| D-0098-template-nid-collision | P0 | Folder nidIndex `0x30` satellite TCs (`0x60D`/`0x60E`/`0x60F`) collide with MS-PST table templates; NBT last-wins empty template; `folders()` / unique-pst verify drop those messages (INC*: −50 in Recoverable Items/Purges) | **Closed in 0098:** `alloc_nid` skips reserved nidIndex `0x30`/`0x33`/`0x34`; `add_node_data` / NBT encode fail closed on duplicate NID | **closed / 0098** |
| D-0100-hn-bitmap-hdr | P3 | HNBITMAPHDR pages (8, 136, 264, …) not implemented in 0100 multi-block HN | 0100/0104 fail-close if a TC heap (recipient or attachment) would land on a bitmap page. INC* 136-row class is expected &lt; 8 pages. Do not implement bitmap pages until a corpus hits the error. | residual / after **0100** |
| D-0100-slblock-nid-order | P3 | Recipient-table SLBLOCK unsorted when cell NIDs exist | **Closed / 0103:** trailing matrix `push` + `add_subnode_leaf` NID-ascending emit-sort (fail closed on duplicates). CI asserts on-disk SLBLOCK order for long-string cell NIDs. | **closed / 0103** |

## From track 0062-ReleaseHardeningRc

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0062-codesign | — | Authenticode for operator-facing RC ZIP | Engineering docs/gates/package may complete; **external counsel handoff blocked** until cert + signed exes | release ops |
| D-0062-audit-rsa | P3 | `rsa` 0.9.x Marvin (RUSTSEC-2023-0071) via `openidconnect` | No fixed upgrade; SSO opt-in path only; ignored in audit/deny with reason | residual / upstream |
| D-0062-audit-quickxml | P3 | `quick-xml` 0.39 advisories via wayland-scanner | Linux GUI transitive only; Windows product path pruned in `deny.toml` targets; cargo-audit ignore until consumers bump | residual / upstream |
| D-0062-audit-warnings | P3 | Unmaintained/unsound warnings (`ttf-parser`, `anyhow`, `memmap2`) | cargo-audit warnings only; not treated as hard RC fail | residual polish |

## From track 0063-SecurityRedTeamFixes

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0063-01 | P3 | Matter passphrase may remain in process **env** after unlock | Same class as D-0057-09; clear-after-read unsafe with concurrent workers. Production unlock/create/change-passphrase **heap** buffers use `Zeroizing<String>` (0063 fix round). | residual polish |
| D-0063-02 | P3 | OIDC SSRF DNS rebinding / resolve-then-connect race | Mitigated by re-validating discovered token/jwks/auth URLs; multi-resolve residual | residual polish |
| D-0063-03 | P3 | XBLOCK assemble hard-cap 64 MiB may reject huge legitimate single assemblies | Streaming redesign needed to raise safely | residual scale |
| D-0063-04 | P3 | `openidconnect::ClientSecret` / bare `String` retains IdP client secret until client Drop; no zeroize API | **P3 residual** (dependency limitation; not a product control gap). Mitigated: `CoreClient` constructed only inside a tight exchange+verify block; route zeroizes local secret after `finish_authorization`. Heap residue only during exchange until allocator reuse. Full zeroize requires upstream `openidconnect` support. | residual / upstream |
| D-0063-05 | P3 | Desk UI passphrase widgets are plain `String` (egui TextEdit) | Cleared after submit; heap residue residual. Production service/CLI unlock paths zeroize. Full zeroizing widgets would need egui field redesign. | residual polish |

## Hygiene

- When closing a deferred row, move it to a short “Fixed” note in the track `review.md` or delete the row.
- Do not park DoD-blocking P0–P2 items here.
