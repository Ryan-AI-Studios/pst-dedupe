//! Job params for `semantic_index`.

use serde::{Deserialize, Serialize};

use crate::error::{Result, SemanticError};

/// Scope: all candidates with non-null `text_sha256`.
pub const SCOPE_ALL: &str = "all";

/// Default mock model id used in CI and when no real model is configured.
pub const DEFAULT_MODEL_ID: &str = "mock:hash_v1";

/// Residual production Candle model id (weights not in git; explicit install).
pub const CANDLE_MODEL_ID_MINILM: &str = "local:minilm-l6-v2";

/// Engine tag embedded in fingerprints for MockEmbedder.
pub const ENGINE_TAG_MOCK: &str = "mock_hash_v1";

/// JSON params for kind `"semantic_index"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticIndexParams {
    /// Active model id (must match embedder). Default: `mock:hash_v1`.
    #[serde(default = "default_model_id")]
    pub model_id: String,
    /// Chunk size in Unicode scalar values (chars). Default 800.
    #[serde(default = "default_chunk_chars")]
    pub chunk_chars: u32,
    /// Overlap in chars between consecutive chunks. Default 120.
    #[serde(default = "default_chunk_overlap")]
    pub chunk_overlap: u32,
    /// Cap chunks per item; drop tail with honesty. Default 48.
    #[serde(default = "default_max_chunks_per_item")]
    pub max_chunks_per_item: u32,
    /// Cap on CAS text bytes loaded per item. Default 200_000.
    #[serde(default = "default_max_text_bytes")]
    pub max_text_bytes: u64,
    /// Fail-closed when more than this many docs would be processed. Default 50_000.
    #[serde(default = "default_max_docs")]
    pub max_docs: u64,
    /// When true, wipe active model namespace + meta then full reindex.
    #[serde(default)]
    pub reset: bool,
    /// Page size for keyset candidate listing / embed batch. Default 16.
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
    /// Candidate scope (P0: `"all"` only).
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_model_id() -> String {
    DEFAULT_MODEL_ID.into()
}
fn default_chunk_chars() -> u32 {
    800
}
fn default_chunk_overlap() -> u32 {
    120
}
fn default_max_chunks_per_item() -> u32 {
    48
}
fn default_max_text_bytes() -> u64 {
    200_000
}
fn default_max_docs() -> u64 {
    50_000
}
fn default_batch_size() -> u32 {
    16
}
fn default_scope() -> String {
    SCOPE_ALL.into()
}

impl Default for SemanticIndexParams {
    fn default() -> Self {
        Self {
            model_id: default_model_id(),
            chunk_chars: default_chunk_chars(),
            chunk_overlap: default_chunk_overlap(),
            max_chunks_per_item: default_max_chunks_per_item(),
            max_text_bytes: default_max_text_bytes(),
            max_docs: default_max_docs(),
            reset: false,
            batch_size: default_batch_size(),
            scope: default_scope(),
        }
    }
}

impl SemanticIndexParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let p: Self =
            serde_json::from_str(json).map_err(|e| SemanticError::InvalidParams(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    /// Validate caps and scope.
    pub fn validate(&self) -> Result<()> {
        if self.scope != SCOPE_ALL {
            return Err(SemanticError::InvalidParams(format!(
                "unknown semantic scope '{}' (expected {SCOPE_ALL})",
                self.scope
            )));
        }
        if self.model_id.trim().is_empty() {
            return Err(SemanticError::InvalidParams(
                "model_id must be non-empty".into(),
            ));
        }
        if self.chunk_chars == 0 {
            return Err(SemanticError::InvalidParams(
                "chunk_chars must be > 0".into(),
            ));
        }
        if self.chunk_overlap >= self.chunk_chars {
            return Err(SemanticError::InvalidParams(format!(
                "chunk_overlap ({}) must be < chunk_chars ({})",
                self.chunk_overlap, self.chunk_chars
            )));
        }
        if self.max_chunks_per_item == 0 {
            return Err(SemanticError::InvalidParams(
                "max_chunks_per_item must be > 0".into(),
            ));
        }
        if self.max_text_bytes == 0 {
            return Err(SemanticError::InvalidParams(
                "max_text_bytes must be > 0".into(),
            ));
        }
        if self.max_docs == 0 {
            return Err(SemanticError::InvalidParams("max_docs must be > 0".into()));
        }
        if self.batch_size == 0 {
            return Err(SemanticError::InvalidParams(
                "batch_size must be > 0".into(),
            ));
        }
        Ok(())
    }

    /// Stable chunk params JSON for fingerprint / matter meta.
    pub fn chunk_params_json(&self) -> Result<String> {
        let v = serde_json::json!({
            "chunk_chars": self.chunk_chars,
            "chunk_overlap": self.chunk_overlap,
            "max_chunks_per_item": self.max_chunks_per_item,
        });
        Ok(serde_json::to_string(&v)?)
    }

    /// Fingerprint: model + dims + chunk params + engine tag.
    pub fn fingerprint(&self, dims: usize, engine_tag: &str) -> String {
        format!(
            "model_id={}|dims={}|chunk_chars={}|chunk_overlap={}|max_chunks={}|engine={}",
            self.model_id.trim(),
            dims,
            self.chunk_chars,
            self.chunk_overlap,
            self.max_chunks_per_item,
            engine_tag
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty() {
        let p = SemanticIndexParams::from_json("{}").unwrap();
        assert_eq!(p.model_id, DEFAULT_MODEL_ID);
        assert_eq!(p.chunk_chars, 800);
        assert_eq!(p.chunk_overlap, 120);
        assert_eq!(p.max_chunks_per_item, 48);
        assert_eq!(p.max_text_bytes, 200_000);
        assert_eq!(p.max_docs, 50_000);
        assert!(!p.reset);
        assert_eq!(p.batch_size, 16);
        assert_eq!(p.scope, "all");
    }

    #[test]
    fn rejects_overlap_ge_chars() {
        let err = SemanticIndexParams::from_json(r#"{"chunk_chars":100,"chunk_overlap":100}"#)
            .unwrap_err();
        assert!(err.to_string().contains("chunk_overlap"));
    }
}
