//! Deterministic bag-of-hash MockEmbedder for CI (no weights).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::normalize::l2_normalize;
use super::Embedder;
use crate::error::Result;
use crate::params::{DEFAULT_MODEL_ID, ENGINE_TAG_MOCK};

/// Default dimensions for [`MockEmbedder`].
pub const MOCK_DIMS: usize = 32;

/// Model id for the mock embedder.
pub const MOCK_MODEL_ID: &str = DEFAULT_MODEL_ID;

/// Deterministic embedding via hashed tokens + char n-grams.
///
/// Same text → same vector. Shared tokens increase cosine similarity so
/// paraphrase-ish tests work without a real model.
#[derive(Debug, Clone)]
pub struct MockEmbedder {
    dims: usize,
    model_id: String,
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self {
            dims: MOCK_DIMS,
            model_id: MOCK_MODEL_ID.into(),
        }
    }
}

impl MockEmbedder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_dims(dims: usize) -> Self {
        Self {
            dims: dims.max(8),
            model_id: MOCK_MODEL_ID.into(),
        }
    }

    /// Deterministic mock with a custom `model_id` (tests multi-namespace).
    pub fn with_model_id(model_id: impl Into<String>) -> Self {
        Self {
            dims: MOCK_DIMS,
            model_id: model_id.into(),
        }
    }

    fn embed_one(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0.0f32; self.dims];
        let lower = text.to_lowercase();
        // Whitespace tokens.
        for token in lower.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation()) {
            if token.is_empty() {
                continue;
            }
            accumulate_token(&mut v, token, 1.0);
        }
        // Char 3-grams for partial token overlap.
        let chars: Vec<char> = lower.chars().filter(|c| !c.is_whitespace()).collect();
        if chars.len() >= 3 {
            for window in chars.windows(3) {
                let gram: String = window.iter().collect();
                accumulate_token(&mut v, &gram, 0.35);
            }
        }
        // Tiny bias so empty text is not the zero vector after normalize skip.
        if v.iter().all(|x| *x == 0.0) {
            v[0] = 1.0;
        }
        l2_normalize(&mut v);
        v
    }
}

fn accumulate_token(v: &mut [f32], token: &str, weight: f32) {
    let dims = v.len();
    let h = stable_hash(token);
    let idx = (h as usize) % dims;
    v[idx] += weight;
    // Secondary bucket reduces collisions and spreads mass.
    let h2 = stable_hash(&(token.to_string() + "#2"));
    let idx2 = (h2 as usize) % dims;
    v[idx2] += weight * 0.5;
}

fn stable_hash(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

impl Embedder for MockEmbedder {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn engine_tag(&self) -> &str {
        ENGINE_TAG_MOCK
    }

    fn embed_passages(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.embed_one(t)).collect())
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        Ok(self.embed_one(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedder::cosine_similarity;

    #[test]
    fn deterministic_and_similar() {
        let e = MockEmbedder::default();
        let a = e.embed_query("fraud investigation bribery").unwrap();
        let b = e.embed_query("fraud investigation bribery").unwrap();
        assert_eq!(a, b);
        let c = e.embed_query("fraud bribery probe").unwrap();
        let d = e
            .embed_query("completely unrelated picnic recipes")
            .unwrap();
        let sim_close = cosine_similarity(&a, &c);
        let sim_far = cosine_similarity(&a, &d);
        assert!(sim_close > sim_far, "close={sim_close} far={sim_far}");
    }
}
