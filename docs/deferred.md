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
| D-0018-03 | — | MAPI recipient table (vs Display* only) | Best-effort DisplayTo/Cc/Bcc P0 | later |
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
| D-0045-02 | — | Cross-process cancel of in-flight job | `job cancel` marks DB; SIGINT cancels in-process runner | residual |
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
| D-0058-01 | — | Desk “Connect to service” UX (URL + login + session actor) | API-only P0; solo Desk unchanged | residual polish |
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
| D-0059-02 | — | Desk browser “Sign in with SSO” UX | Builds on D-0058-01 Connect | residual polish |
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
| D-0060-02 | — | Desk produce profile dropdown | CLI + headless DoD met; Desk uses default profile + pack-aware soft-gate | residual polish |
| D-0060-03 | — | Auto suggest next Bates (MAX prefix) | Start still explicit required | residual |
| D-0060-04 | — | Image + OPT/LFP production profiles | D-0040-01; name_by_bates extends to images | residual |
| D-0060-05 | — | Full Relativity load-file suite | Alias map only P0 | residual |
| D-0060-06 | — | Firm-wide profile pack sync | Matter-local upsert is enough P0 | residual |
| D-0060-07 | — | UK/EU/AU full protocol packs | Beyond template tags / jurisdiction_tag | residual |
| D-0060-08 | P3 | Volume README.txt hardcodes DATA/NATIVES/TEXT + UTC wording | Profile layout/date may differ; DAT is authoritative | residual polish |
| D-0041-05 | — | Multi-jurisdiction QC packs | **Partial close in 0060** (named packs + profile binding) | residual firm packs |

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

## Hygiene

- When closing a deferred row, move it to a short “Fixed” note in the track `review.md` or delete the row.
- Do not park DoD-blocking P0–P2 items here.
