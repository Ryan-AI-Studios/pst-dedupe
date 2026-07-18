//! # matter-search
//!
//! Per-matter **full-text keyword search** over extracted item text using
//! embedded **Tantivy 0.26.x** (track **0029**).
//!
//! | Store | Owns |
//! |---|---|
//! | SQLite (`matter-core`) | Items, codes, filters, FTS bookkeeping (`fts_*`) |
//! | **Tantivy** (`index/`) | Tokenized subject / body / path / attach_names |
//!
//! ## Rules
//!
//! - **Delete-before-add:** always `delete_term(item_id)` then `add_document`
//! - **Query de-dupe:** HashSet unique `item_id` on search results
//! - **Windows rebuild:** drop all Index/Reader handles before `remove_dir_all`
//! - **No FTS5 primary** — SQLite stays metadata-only
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
pub mod params;
pub mod query;
pub mod run;
pub mod schema;

pub use compose::{compose_keyword_filter, compose_with_hits, DEFAULT_FTS_FETCH_LIMIT};
pub use error::{Result, SearchError};
pub use index::{
    delete_then_add, remove_index_dir, MatterIndex, DEFAULT_WRITER_HEAP_BYTES, INDEX_DIR_NAME,
};
pub use params::FtsIndexParams;
pub use query::{search_index, search_keyword, KeywordHits, KeywordQuery};
pub use run::{run_fts_index, FtsOutcome, FtsSummary, FTS_STAGE, JOB_KIND_FTS_INDEX};
pub use schema::FtsSchema;
