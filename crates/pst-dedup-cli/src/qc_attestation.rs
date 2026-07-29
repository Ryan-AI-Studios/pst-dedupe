//! Operator attestation record (`qc_attestation_v1`) — track 0080 §3.7.
//!
//! **Human-signed only.** The tool loads/records a file an operator wrote; it
//! never invents a self-attestation that a human opened the PST in Outlook.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Schema id for attestation JSON.
pub const QC_ATTESTATION_SCHEMA: &str = "qc_attestation_v1";

/// Signed-by-a-human block recording a manual client open.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QcAttestationV1 {
    pub schema: String,
    /// Classic Outlook / new Outlook import / third-party tool name.
    pub tool: String,
    /// Tool version string as reported by the operator.
    pub version: String,
    /// ISO-8601 or free-form date of the open.
    pub date: String,
    /// Operator identity (name/email).
    pub operator: String,
    /// Volume paths or indices opened.
    pub volumes_opened: Vec<String>,
    /// Approximate messages observed.
    pub messages_seen: Option<u64>,
    /// Whether an attachment was opened successfully.
    pub attachment_opened_ok: Option<bool>,
    /// Free-text notes.
    #[serde(default)]
    pub notes: String,
}

impl QcAttestationV1 {
    /// Validate schema field.
    pub fn is_valid_schema(&self) -> bool {
        self.schema == QC_ATTESTATION_SCHEMA
    }
}

/// Load attestation JSON from disk. Returns `None` if missing.
///
/// Errors if the file exists but is not valid JSON or wrong schema.
pub fn load_attestation(path: &Path) -> Result<Option<QcAttestationV1>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let v: QcAttestationV1 =
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
    if !v.is_valid_schema() {
        return Err(format!(
            "attestation schema {:?} (expected {QC_ATTESTATION_SCHEMA})",
            v.schema
        ));
    }
    Ok(Some(v))
}

/// Write an operator-supplied attestation. **Callers must not fabricate fields.**
pub fn write_attestation(path: &Path, attestation: &QcAttestationV1) -> Result<(), String> {
    if !attestation.is_valid_schema() {
        return Err(format!(
            "refusing to write attestation with schema {:?}",
            attestation.schema
        ));
    }
    let json = serde_json::to_string_pretty(attestation)
        .map_err(|e| format!("serialize attestation: {e}"))?;
    fs::write(path, json).map_err(|e| format!("write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_missing_is_none() {
        let dir = TempDir::new().expect("tmp");
        assert!(load_attestation(&dir.path().join("nope.json"))
            .expect("ok")
            .is_none());
    }

    #[test]
    fn round_trip() {
        let dir = TempDir::new().expect("tmp");
        let path = dir.path().join("attestation.json");
        let a = QcAttestationV1 {
            schema: QC_ATTESTATION_SCHEMA.into(),
            tool: "classic Outlook".into(),
            version: "16.0".into(),
            date: "2026-07-28".into(),
            operator: "qa@example.com".into(),
            volumes_opened: vec!["unique.pst".into()],
            messages_seen: Some(3),
            attachment_opened_ok: Some(true),
            notes: "opened fine".into(),
        };
        write_attestation(&path, &a).expect("write");
        let loaded = load_attestation(&path).expect("load").expect("some");
        assert_eq!(loaded, a);
    }

    #[test]
    fn refuse_wrong_schema_write() {
        let dir = TempDir::new().expect("tmp");
        let bad = QcAttestationV1 {
            schema: "not_attestation".into(),
            tool: "x".into(),
            version: "1".into(),
            date: "d".into(),
            operator: "o".into(),
            volumes_opened: vec![],
            messages_seen: None,
            attachment_opened_ok: None,
            notes: String::new(),
        };
        assert!(write_attestation(&dir.path().join("a.json"), &bad).is_err());
    }
}
