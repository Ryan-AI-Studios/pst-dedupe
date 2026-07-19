//! Job params for `classify`.

use serde::{Deserialize, Serialize};

/// JSON params for kind `"classify"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassifyParams {
    /// Re-run full pipeline even for decisive taxonomy_v1 rows (default false).
    #[serde(default)]
    pub force: bool,
    /// Items between cancel checks / checkpoint writes (default 100).
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Read ≤64 KiB CAS head for magic when native present (default true).
    #[serde(default = "default_true")]
    pub use_magic: bool,
    /// Only process items with `in_review = 1` (default false).
    #[serde(default)]
    pub in_review_only: bool,
    /// Keep non-legacy closed-set categories when not force (default true).
    #[serde(default = "default_true")]
    pub respect_extractor_refine: bool,
}

fn default_batch_size() -> usize {
    100
}

fn default_true() -> bool {
    true
}

impl Default for ClassifyParams {
    fn default() -> Self {
        Self {
            force: false,
            batch_size: default_batch_size(),
            use_magic: true,
            in_review_only: false,
            respect_extractor_refine: true,
        }
    }
}

impl ClassifyParams {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.batch_size == 0 {
            return Err("batch_size must be >= 1".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let p = ClassifyParams::from_json("{}").unwrap();
        assert!(!p.force);
        assert_eq!(p.batch_size, 100);
        assert!(p.use_magic);
        assert!(!p.in_review_only);
        assert!(p.respect_extractor_refine);
        p.validate().unwrap();
    }
}
