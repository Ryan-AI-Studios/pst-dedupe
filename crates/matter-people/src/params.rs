//! Job params for `people_graph`.

use serde::{Deserialize, Serialize};

use crate::error::{PeopleError, Result};

/// Scope: all items with participant address fields.
pub const SCOPE_ALL: &str = "all";

/// Timeline grain: calendar day.
pub const GRAIN_DAY: &str = "day";
/// Timeline grain: ISO week.
pub const GRAIN_WEEK: &str = "week";

/// JSON params for kind `"people_graph"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeopleGraphParams {
    /// Candidate scope (P0: `"all"` only).
    #[serde(default = "default_scope")]
    pub scope: String,
    /// Include body entity email hits.
    ///
    /// Default **false** (headers-only). When `true`, params validation **fails closed**
    /// — entity-body email join is unsupported/deferred (not a silent no-op).
    #[serde(default)]
    pub include_entity_emails: bool,
    /// Timeline grain: `day` | `week`.
    #[serde(default = "default_grain")]
    pub grain: String,
    /// When true, wipe people-graph tables then rebuild.
    ///
    /// Default **true** (desk also defaults `reset:true`). With `reset:false`, a
    /// complete graph whose fingerprint matches engine_version+params is skipped;
    /// that soft skip does **not** detect item inventory changes (residual).
    #[serde(default = "default_reset")]
    pub reset: bool,
    /// Pass-1 keyset page size.
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
    /// Cap recipients expanded per item (to+cc+bcc).
    #[serde(default = "default_max_recipients")]
    pub max_recipients_per_item: u32,
}

fn default_scope() -> String {
    SCOPE_ALL.into()
}

fn default_grain() -> String {
    GRAIN_DAY.into()
}

fn default_reset() -> bool {
    true
}

fn default_batch_size() -> u32 {
    200
}

fn default_max_recipients() -> u32 {
    200
}

impl Default for PeopleGraphParams {
    fn default() -> Self {
        Self {
            scope: default_scope(),
            include_entity_emails: false,
            grain: default_grain(),
            reset: default_reset(),
            batch_size: default_batch_size(),
            max_recipients_per_item: default_max_recipients(),
        }
    }
}

impl PeopleGraphParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let p: Self =
            serde_json::from_str(json).map_err(|e| PeopleError::InvalidParams(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    /// Validate scope, grain, caps, and residual flags.
    pub fn validate(&self) -> Result<()> {
        if self.scope != SCOPE_ALL {
            return Err(PeopleError::InvalidParams(format!(
                "unknown people_graph scope '{}' (expected {SCOPE_ALL})",
                self.scope
            )));
        }
        if self.grain != GRAIN_DAY && self.grain != GRAIN_WEEK {
            return Err(PeopleError::InvalidParams(format!(
                "unknown people_graph grain '{}' (expected {GRAIN_DAY}|{GRAIN_WEEK})",
                self.grain
            )));
        }
        if self.include_entity_emails {
            return Err(PeopleError::InvalidParams(
                "include_entity_emails=true is not supported yet (entity-body email join is deferred); \
                 use include_entity_emails=false (default) for headers-only people_graph"
                    .into(),
            ));
        }
        if self.batch_size == 0 {
            return Err(PeopleError::InvalidParams("batch_size must be > 0".into()));
        }
        if self.max_recipients_per_item == 0 {
            return Err(PeopleError::InvalidParams(
                "max_recipients_per_item must be > 0".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty() {
        let p = PeopleGraphParams::from_json("{}").unwrap();
        assert_eq!(p.scope, "all");
        assert!(!p.include_entity_emails);
        assert_eq!(p.grain, "day");
        assert!(p.reset);
        assert_eq!(p.batch_size, 200);
        assert_eq!(p.max_recipients_per_item, 200);
    }

    #[test]
    fn rejects_bad_grain() {
        let err = PeopleGraphParams::from_json(r#"{"grain":"month"}"#).unwrap_err();
        assert!(err.to_string().contains("grain"));
    }

    #[test]
    fn include_entity_emails_true_fails_closed() {
        let err = PeopleGraphParams::from_json(r#"{"include_entity_emails":true}"#).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("include_entity_emails") && msg.contains("not supported"),
            "expected fail-closed message, got: {msg}"
        );
    }

    #[test]
    fn include_entity_emails_false_ok() {
        let p = PeopleGraphParams::from_json(r#"{"include_entity_emails":false}"#).unwrap();
        assert!(!p.include_entity_emails);
        p.validate().unwrap();
    }
}
