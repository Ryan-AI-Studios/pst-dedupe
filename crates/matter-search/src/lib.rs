//! # matter-search
//!
//! Per-matter **full-text keyword search** over extracted item text using
//! embedded **Tantivy 0.26.x** (track **0029** + multilingual packs **0054**).
//!
//! | Store | Owns |
//! |---|---|
//! | SQLite (`matter-core`) | Items, codes, filters, FTS bookkeeping (`fts_*`), lang pack |
//! | **Tantivy** (`index/`) | Tokenized subject / body / path / attach_names |
//!
//! ## Rules
//!
//! - **Delete-before-add:** always `delete_term(item_id)` then `add_document`
//! - **Query de-dupe:** HashSet unique `item_id` on search results
//! - **Windows rebuild:** drop all Index/Reader handles before `remove_dir_all`
//! - **No FTS5 primary** — SQLite stays metadata-only
//! - **Pack stale gate:** fingerprint mismatch → hard error (`fts_lang_pack_stale`)
//! - **CJK consecutive query → phrase** (positional), not free AND of unigrams
//!
//! ## Default Tantivy features
//!
//! Workspace pins `tantivy = "0.26"` with **default features** (mmap, stopwords,
//! stemmer tokenizers). Document dialect in the crate README.
//!
//! ## Identity
//!
//! Never delete items or CAS blobs. Never write full body into Tantivy STORED
//! fields (body is re-read from CAS for the viewer).

#![forbid(unsafe_code)]

pub mod compose;
pub mod error;
pub mod index;
pub mod pack;
pub mod params;
pub mod query;
pub mod run;
pub mod schema;
pub mod tokenizer;

pub use compose::{compose_keyword_filter, compose_with_hits};
pub use error::{Result, SearchError, CODE_FTS_LANG_PACK_STALE};
pub use index::{
    delete_then_add, register_pack_tokenizers, remove_index_dir, MatterIndex,
    DEFAULT_WRITER_HEAP_BYTES, INDEX_DIR_NAME,
};
pub use pack::{
    fingerprint_for_pack_id, LangPack, CJK_HYBRID_TOKENIZER_ID, CJK_MAX_GRAM, CJK_MIN_GRAM,
    FTS_SCHEMA_ID,
};
pub use params::FtsIndexParams;
pub use query::{
    search_index, search_index_with_pack, search_keyword, search_keyword_for_matter,
    search_keyword_with_pack, KeywordHits, KeywordQuery, DEFAULT_FTS_FETCH_LIMIT,
};
pub use run::{run_fts_index, FtsOutcome, FtsSummary, FTS_STAGE, JOB_KIND_FTS_INDEX};
pub use schema::FtsSchema;
pub use tokenizer::{emit_hybrid_tokens, rewrite_cjk_query_phrases, HybridCjkTokenizer};
