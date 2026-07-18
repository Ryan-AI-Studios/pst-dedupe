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
| D-0016-09 | — | CLI `ingest` smoke subcommand | Optional nice-to-have | future |

## From track 0017-NormalizedItem

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0017-01 | P3 | Unique index on `items(source_id, path)` | Optional in DoD; resume remains app-level | future polish |
| D-0017-02 | P3 | Formal SQLite FK on `parent_item_id` | App-enforced; ALTER cannot add FK cleanly | later migration if needed |
| D-0017-03 | — | Relational `item_participants` | JSON P0 by design; Tantivy/graph later | **0029 / 0038 / 0047** |
| D-0017-04 | — | Body-to-CAS promote helper (`text_sha256`) | **Done in 0018** (body → CAS + column) | — |
| D-0017-05 | — | Bulk rehash / fill from PST | Extract fill in 0018; runner in 0019 | bulk job polish later |

## From track 0018-PstExtractorAdapter

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0018-01 | P3 | Attach path may materialize large subnode `Vec` before stream switch | Primary path streams; residual fallback | future polish |
| D-0018-02 | — | EML as native identity | Never; production EML export separate | **0040** |
| D-0018-03 | — | MAPI recipient table (vs Display* only) | Best-effort DisplayTo/Cc/Bcc P0 | later |
| D-0018-04 | — | Process runner / progress UI | Runner **done in 0019**; Desk UI progress | **0020** |

## From track 0019-ProcessJobRunner

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0019-01 | — | Multi-job parallel stages per matter | P0 single-flight only | future / **0044** |
| D-0019-02 | — | Full CLI `job run|resume|cancel` | `examples/run_job.rs` smoke only | future |
| D-0019-03 | P3 | Extract cancel→resume via runner | Ingest path proven; extract fixture success proven | future polish |
| D-0019-04 | — | Rayon pure-CPU stages without Matter | Forbidden for Matter path P0 | later |

| D-0018-05 | — | CLI `extract` subcommand | Optional | future |

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
| D-0027-01 | — | Privilege log export / 502(d) workflow | P0: Privilege **code** only (membership); not full log | **0031** |
| D-0027-02 | — | Filter list / saved search by code | **Done in 0028** (code any_of/none_of/missing + desk chips) | — |
| D-0027-03 | — | Auto-propagate to near-dup / full thread | Never default; family = parent+all children only | residual / **0056** |
| D-0027-04 | — | QC sampling reports / multi-reviewer lock | Single-desk P0 | later / **0058** |
| D-0027-05 | P3 | Full GUI smoke for coding panel / batch / digits path | Automated matter-core + desk unit/integration tests; operator smoke local | operator / polish |
| D-0027-06 | — | Production export of coded subsets | Membership only in 0027 | **0040** |

## From track 0028-FiltersSavedSearch

| ID | Severity | Item | Notes | Owner |
|---|---|---|---|---|
| D-0028-01 | P3 | Keyset/cursor pagination if deep OFFSET still slow | P0: LIMIT/OFFSET + partial `idx_items_review_list_order`; Codex residual | residual scale |
| D-0028-02 | — | Nested saved-search-as-condition / deep OR builder | P0: flat AND only (Relativity nesting timeout risk) | residual |
| D-0028-03 | P3 | Full GUI smoke for filter bar / saved search / Load more | Automated matter-core + desk unit/integration; operator smoke local | operator / polish |
| D-0028-04 | — | Body keyword in FilterSpec | Metadata only; FTS → **0029** | **0029** |
| D-0026-01 | P3 | Large corpus paging | **Improved in 0028**: filtered count + Load more + compound list index; unfiltered Load more too | residual if multi-million |

## Hygiene

- When closing a deferred row, move it to a short “Fixed” note in the track `review.md` or delete the row.
- Do not park DoD-blocking P0–P2 items here.
