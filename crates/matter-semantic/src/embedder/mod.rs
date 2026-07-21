//! Embedder trait + MockEmbedder (+ optional fail-closed local backend).

mod mock;
mod normalize;

#[cfg(feature = "semantic-candle")]
mod local;

pub use mock::{MockEmbedder, MOCK_DIMS, MOCK_MODEL_ID};
pub use normalize::{cosine_similarity, l2_normalize, l2_normalize_owned};

use crate::error::Result;

/// Local embedding backend (passage + query).
///
/// Implementations **must** L2-normalize outputs so cosine = dot product.
pub trait Embedder: Send + Sync {
    /// Stable model id (e.g. `mock:hash_v1`, `local:minilm-l6-v2`).
    fn model_id(&self) -> &str;

    /// Output dimensionality.
    fn dimensions(&self) -> usize;

    /// Engine tag for fingerprints (e.g. `mock_hash_v1`).
    fn engine_tag(&self) -> &str;

    /// Embed one or more passage texts (document chunks).
    fn embed_passages(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;

    /// Embed a single query string.
    fn embed_query(&self, text: &str) -> Result<Vec<f32>>;
}

/// Resolve the default embedder for a model_id (mock only in default features).
pub fn embedder_for_model_id(model_id: &str) -> Result<Box<dyn Embedder>> {
    let id = model_id.trim();
    if id.is_empty() || id == MOCK_MODEL_ID {
        return Ok(Box::new(MockEmbedder::default()));
    }
    if id.starts_with("mock:") {
        // Distinct mock model_ids get their own id string (namespace isolation tests).
        return Ok(Box::new(MockEmbedder::with_model_id(id)));
    }
    #[cfg(feature = "semantic-candle")]
    {
        return Ok(Box::new(local::LocalEmbedder::try_new(id)?));
    }
    #[cfg(not(feature = "semantic-candle"))]
    {
        Err(crate::error::SemanticError::Embedder(format!(
            "model '{id}' requires a local embedder backend; default build only includes MockEmbedder (`mock:hash_v1`). \
             Enable feature `semantic-candle` and install weights, or set model_id to mock:hash_v1"
        )))
    }
}
