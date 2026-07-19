//! Job params for `pdf_extract`.

use serde::{Deserialize, Serialize};

/// JSON params for kind `"pdf_extract"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfExtractParams {
    /// Re-extract even when already extracted for the same native (default false).
    #[serde(default)]
    pub force: bool,
    /// Items between cancel checks / checkpoint writes (default 50).
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

fn default_batch_size() -> usize {
    50
}

impl Default for PdfExtractParams {
    fn default() -> Self {
        Self {
            force: false,
            batch_size: default_batch_size(),
        }
    }
}

impl PdfExtractParams {
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
        let p = PdfExtractParams::from_json("{}").unwrap();
        assert!(!p.force);
        assert_eq!(p.batch_size, 50);
        p.validate().unwrap();
    }
}
