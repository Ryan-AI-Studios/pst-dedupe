//! Job params for `office_extract`.

use serde::{Deserialize, Serialize};

/// JSON params for kind `"office_extract"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeExtractParams {
    /// Re-extract even when text already set for the same native (default false).
    #[serde(default)]
    pub force: bool,
    /// Items between cancel checks / checkpoint writes (default 50).
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Formats to process (default all three).
    #[serde(default = "default_formats")]
    pub formats: Vec<String>,
}

fn default_batch_size() -> usize {
    50
}

fn default_formats() -> Vec<String> {
    vec!["docx".into(), "xlsx".into(), "pptx".into()]
}

impl Default for OfficeExtractParams {
    fn default() -> Self {
        Self {
            force: false,
            batch_size: default_batch_size(),
            formats: default_formats(),
        }
    }
}

impl OfficeExtractParams {
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
        if self.formats.is_empty() {
            return Err("formats must be non-empty".into());
        }
        for f in &self.formats {
            let l = f.to_ascii_lowercase();
            if !matches!(l.as_str(), "docx" | "xlsx" | "pptx") {
                return Err(format!("unsupported format '{f}' (P0: docx/xlsx/pptx)"));
            }
        }
        Ok(())
    }

    pub fn allows_format(&self, format: &str) -> bool {
        let l = format.to_ascii_lowercase();
        self.formats.iter().any(|f| f.eq_ignore_ascii_case(&l))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let p = OfficeExtractParams::from_json("{}").unwrap();
        assert!(!p.force);
        assert_eq!(p.batch_size, 50);
        assert_eq!(p.formats.len(), 3);
        p.validate().unwrap();
    }
}
