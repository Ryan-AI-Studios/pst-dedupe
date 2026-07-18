//! Job params for matter-level cull.

use serde::{Deserialize, Serialize};

use crate::error::{CullError, Result};
use crate::presets::{builtin_rules, PRESET_UNIQUE_ONLY};
use crate::rules::CullRules;

/// JSON params for kind `"cull"`.
///
/// Rules resolution order:
/// 1. Inline `rules` object (when present)
/// 2. `preset_id` → load from matter `cull_presets` table (resolved in run)
/// 3. `preset_name` → built-in or DB name (DB resolved in run)
/// 4. Default: built-in `unique_only`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CullParams {
    /// Built-in or user preset name (e.g. `"unique_only"`).
    #[serde(default)]
    pub preset_name: Option<String>,
    /// User preset id from `cull_presets`.
    #[serde(default)]
    pub preset_id: Option<String>,
    /// Inline rules object (takes precedence when present).
    #[serde(default)]
    pub rules: Option<CullRules>,
    /// Clear prior cull result cols then recompute (default true).
    #[serde(default = "default_true")]
    pub reset: bool,
    /// Checkpoint / write batch size (default 500).
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
}

fn default_true() -> bool {
    true
}

fn default_batch_size() -> u64 {
    500
}

impl Default for CullParams {
    fn default() -> Self {
        Self {
            preset_name: Some(PRESET_UNIQUE_ONLY.into()),
            preset_id: None,
            rules: None,
            reset: true,
            batch_size: default_batch_size(),
        }
    }
}

impl CullParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let p: Self =
            serde_json::from_str(json).map_err(|e| CullError::InvalidParams(e.to_string()))?;
        p.validate_shape()?;
        Ok(p)
    }

    /// Validate batch size and mutually exclusive sources (soft).
    pub fn validate_shape(&self) -> Result<()> {
        if self.batch_size == 0 {
            return Err(CullError::InvalidParams("batch_size must be >= 1".into()));
        }
        if let Some(ref rules) = self.rules {
            rules.validate()?;
        }
        Ok(())
    }

    /// Resolve built-in or inline rules without DB access.
    ///
    /// Returns `(rules, preset_name_for_audit, preset_id_for_audit)`.
    /// When only `preset_id` is set, returns `None` so the runner can load from DB.
    #[allow(clippy::type_complexity)]
    pub fn try_resolve_local(&self) -> Result<Option<(CullRules, Option<String>, Option<String>)>> {
        if let Some(ref rules) = self.rules {
            rules.validate()?;
            return Ok(Some((
                rules.clone(),
                self.preset_name.clone(),
                self.preset_id.clone(),
            )));
        }
        if self.preset_id.is_some() {
            // Needs DB.
            return Ok(None);
        }
        let name = self.preset_name.as_deref().unwrap_or(PRESET_UNIQUE_ONLY);
        if let Some(rules) = builtin_rules(name) {
            rules.validate()?;
            return Ok(Some((rules, Some(name.to_string()), None)));
        }
        // Non-builtin name without id — needs DB lookup by name in run.
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty() {
        let p = CullParams::from_json("{}").unwrap();
        assert!(p.reset);
        assert_eq!(p.batch_size, 500);
        assert!(p.rules.is_none());
    }

    #[test]
    fn preset_name_unique_only() {
        let p = CullParams::from_json(r#"{"preset_name":"unique_only"}"#).unwrap();
        let resolved = p.try_resolve_local().unwrap().unwrap();
        assert!(resolved.0.exclude_exact_duplicates);
    }
}
