//! # matter-semantic
//!
//! Opt-in **local semantic search** (track **0050**):
//!
//! - Keyword FTS (Tantivy / **0029**) remains primary — this crate is **additive**
//! - Default OFF; **local only**; no cloud; no silent model download
//! - Chunk + overlap; retrieval unit = chunk → item
//! - L2 normalize + cosine
//! - **Pre-filter** FilterSpec → then cosine (never post-filter global top_k)
//! - **Group-by item max score → top_n items** (never chunk-limit-only)
//! - Store at `{matter}/semantic/{sanitized_model_id}/`
//! - Job kind [`JOB_KIND_SEMANTIC_INDEX`] (`semantic_index`)
//! - CI: [`MockEmbedder`] (`mock:hash_v1`); optional `semantic-candle` fail-closed stub
//!
//! ## Honesty
//!
//! - Semantic ≠ Boolean keyword precision; false friends expected
//! - English-centric small models mis-rank other languages (**0054**)
//! - Filters apply **before** ranking — empty under a filter means no in-set hit
//! - Offline only after local model available; mock works without weights

#![forbid(unsafe_code)]

pub mod chunk;
pub mod embedder;
pub mod error;
pub mod params;
pub mod query;
pub mod run;
pub mod store;

pub use chunk::{chunk_text, ChunkResult, TextChunk};
pub use embedder::{
    cosine_similarity, embedder_for_model_id, l2_normalize, l2_normalize_owned, Embedder,
    MockEmbedder, MOCK_DIMS, MOCK_MODEL_ID,
};
pub use error::{Result, SemanticError};
pub use params::{
    SemanticIndexParams, CANDLE_MODEL_ID_MINILM, DEFAULT_MODEL_ID, ENGINE_TAG_MOCK, SCOPE_ALL,
};
pub use query::{
    search_semantic, search_semantic_capped, SemanticHit, SemanticQuery, SemanticSearchResult,
    DEFAULT_MAX_ELIGIBLE_ITEMS,
};
pub use run::{
    run_semantic_index, run_semantic_index_with_embedder, SemanticOutcome, SemanticReport,
    SemanticSummary, JOB_KIND_SEMANTIC_INDEX, SEMANTIC_STAGE,
};
pub use store::{
    namespace_dir, sanitize_model_id, ItemVectorFile, SemanticStore, StoreMeta, StoredChunk,
    SEMANTIC_DIR_NAME, STORE_FORMAT_VERSION,
};
