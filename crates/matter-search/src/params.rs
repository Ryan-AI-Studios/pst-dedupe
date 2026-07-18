//! Job params for `fts_index`.

use serde::{Deserialize, Serialize};

use crate::index::DEFAULT_WRITER_HEAP_BYTES;

/// JSON params for kind `"fts_index"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FtsIndexParams {
    /// Recreate empty index + clear fts_* + full rebuild (default false).
    #[serde(default)]
    pub reset: bool,
    /// Checkpoint / write batch size (default 100).
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Eligibility scope — P0 only `"all_with_text"`.
    #[serde(default = "default_scope")]
    pub scope: String,
    /// IndexWriter heap budget in bytes (default ~50 MiB).
    #[serde(default = "default_writer_heap")]
    pub writer_heap_bytes: usize,
}

fn default_batch_size() -> usize {
    100
}

fn default_scope() -> String {
    "all_with_text".into()
}

fn default_writer_heap() -> usize {
    DEFAULT_WRITER_HEAP_BYTES
}

impl Default for FtsIndexParams {
    fn default() -> Self {
        Self {
            reset: false,
            batch_size: default_batch_size(),
            scope: default_scope(),
            writer_heap_bytes: default_writer_heap(),
        }
    }
}

impl FtsIndexParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json)
    }

    /// Validate batch size, scope, heap.
    pub fn validate(&self) -> Result<(), String> {
        if self.batch_size == 0 {
            return Err("batch_size must be >= 1".into());
        }
        if self.scope != "all_with_text" {
            return Err(format!(
                "unsupported scope '{}' (P0 only supports all_with_text)",
                self.scope
            ));
        }
        if self.writer_heap_bytes < 15_000_000 {
            return Err("writer_heap_bytes must be >= 15_000_000".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty_object() {
        let p = FtsIndexParams::from_json("{}").unwrap();
        assert!(!p.reset);
        assert_eq!(p.batch_size, 100);
        assert_eq!(p.scope, "all_with_text");
        assert_eq!(p.writer_heap_bytes, DEFAULT_WRITER_HEAP_BYTES);
        p.validate().unwrap();
    }

    #[test]
    fn rejects_zero_batch() {
        let p = FtsIndexParams {
            batch_size: 0,
            ..Default::default()
        };
        assert!(p.validate().is_err());
    }
}
