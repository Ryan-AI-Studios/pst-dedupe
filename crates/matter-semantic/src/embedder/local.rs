//! Fail-closed local embedder stub behind `semantic-candle`.
//!
//! Full MiniLM Candle load is residual; this module documents the model path
//! contract and refuses to run without installed weights (no silent download).

use super::Embedder;
use crate::error::{Result, SemanticError};
use crate::params::CANDLE_MODEL_ID_MINILM;

/// Engine tag for the local/Candle path.
pub const ENGINE_TAG_LOCAL: &str = "candle_stub_v0";

/// Placeholder local embedder — always fails closed until weights + Candle load
/// are implemented (P0 uses MockEmbedder).
pub struct LocalEmbedder {
    model_id: String,
}

impl LocalEmbedder {
    /// Construct only when a documented model path exists (P0: always fail).
    pub fn try_new(model_id: &str) -> Result<Self> {
        let id = model_id.trim();
        // Fail closed: no silent download, no weights in git.
        Err(SemanticError::Embedder(format!(
            "local embedder for '{id}' is not installed. \
             Place model weights via explicit operator install (no silent download). \
             Production residual model id: {CANDLE_MODEL_ID_MINILM}. \
             For CI/tests use model_id '{mock}' (MockEmbedder).",
            mock = crate::embedder::MOCK_MODEL_ID
        )))
    }
}

impl Embedder for LocalEmbedder {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dimensions(&self) -> usize {
        384
    }

    fn engine_tag(&self) -> &str {
        ENGINE_TAG_LOCAL
    }

    fn embed_passages(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Err(SemanticError::Embedder(
            "LocalEmbedder not loaded (weights missing)".into(),
        ))
    }

    fn embed_query(&self, _text: &str) -> Result<Vec<f32>> {
        Err(SemanticError::Embedder(
            "LocalEmbedder not loaded (weights missing)".into(),
        ))
    }
}
