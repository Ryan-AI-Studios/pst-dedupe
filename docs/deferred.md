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

## Hygiene

- When closing a deferred row, move it to a short “Fixed” note in the track `review.md` or delete the row.
- Do not park DoD-blocking P0–P2 items here.
