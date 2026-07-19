//! Enum-safe DAT column map (allowlist only — never SQL identifiers).

use std::collections::HashMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{GapError, Result};

/// Hardcoded target fields for DAT → gap_expected_docs mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappedField {
    ControlNumber,
    Sha256,
    MessageId,
    ItemId,
    LogicalHash,
    Custodian,
    FileName,
    FileExt,
    FileCategory,
    MimeType,
    DateSent,
    DateReceived,
    DateCreated,
}

impl MappedField {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ControlNumber => "control_number",
            Self::Sha256 => "sha256",
            Self::MessageId => "message_id",
            Self::ItemId => "item_id",
            Self::LogicalHash => "logical_hash",
            Self::Custodian => "custodian",
            Self::FileName => "file_name",
            Self::FileExt => "file_ext",
            Self::FileCategory => "file_category",
            Self::MimeType => "mime_type",
            Self::DateSent => "date_sent",
            Self::DateReceived => "date_received",
            Self::DateCreated => "date_created",
        }
    }
}

impl FromStr for MappedField {
    type Err = GapError;

    fn from_str(s: &str) -> Result<Self> {
        let key = s.trim().to_ascii_lowercase().replace('-', "_");
        match key.as_str() {
            "control_number" | "controlnumber" => Ok(Self::ControlNumber),
            "sha256" | "native_sha256" | "hash" => Ok(Self::Sha256),
            "message_id" | "messageid" | "mid" => Ok(Self::MessageId),
            "item_id" | "itemid" => Ok(Self::ItemId),
            "logical_hash" | "logicalhash" => Ok(Self::LogicalHash),
            "custodian" => Ok(Self::Custodian),
            "file_name" | "filename" => Ok(Self::FileName),
            "file_ext" | "fileext" | "ext" => Ok(Self::FileExt),
            "file_category" | "filecategory" | "category" => Ok(Self::FileCategory),
            "mime_type" | "mimetype" | "mime" => Ok(Self::MimeType),
            "date_sent" | "datesent" | "sent_at" => Ok(Self::DateSent),
            "date_received" | "datereceived" | "received_at" => Ok(Self::DateReceived),
            "date_created" | "datecreated" | "created_at" => Ok(Self::DateCreated),
            other => Err(GapError::InvalidColumnMap(format!(
                "unknown MappedField target '{other}'"
            ))),
        }
    }
}

/// Header name (after de-qualify) → [`MappedField`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatColumnMap {
    /// Original header → field.
    pub map: HashMap<String, MappedField>,
}

impl DatColumnMap {
    /// Default map for matter_produce_v1 / 0040 DAT field names.
    ///
    /// Note: produce DAT currently has no MESSAGE_ID column — MessageId is optional.
    pub fn default_produce_v1() -> Self {
        let pairs = [
            ("CONTROL_NUMBER", MappedField::ControlNumber),
            ("SHA256", MappedField::Sha256),
            ("ITEM_ID", MappedField::ItemId),
            ("CUSTODIAN", MappedField::Custodian),
            ("FILE_NAME", MappedField::FileName),
            ("FILE_EXT", MappedField::FileExt),
            ("FILE_CATEGORY", MappedField::FileCategory),
            ("MIME_TYPE", MappedField::MimeType),
            ("DATE_SENT", MappedField::DateSent),
            ("DATE_RECEIVED", MappedField::DateReceived),
            ("DATE_CREATED", MappedField::DateCreated),
            // Optional extras if present (foreign or future produce):
            ("MESSAGE_ID", MappedField::MessageId),
            ("LOGICAL_HASH", MappedField::LogicalHash),
        ];
        let mut map = HashMap::new();
        for (h, f) in pairs {
            map.insert(h.to_string(), f);
        }
        Self { map }
    }

    /// Parse a JSON object of header → target field string (enum allowlist only).
    pub fn from_json_map(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default_produce_v1());
        }
        let raw: HashMap<String, String> = serde_json::from_str(json)
            .map_err(|e| GapError::InvalidColumnMap(format!("column map JSON: {e}")))?;
        Self::from_string_map(&raw)
    }

    /// Build from string→string map; unknown targets → Error.
    pub fn from_string_map(raw: &HashMap<String, String>) -> Result<Self> {
        let mut map = HashMap::new();
        for (header, target) in raw {
            let field = MappedField::from_str(target)?;
            map.insert(header.clone(), field);
        }
        if map.is_empty() {
            return Err(GapError::InvalidColumnMap(
                "column map must contain at least one mapping".into(),
            ));
        }
        Ok(Self { map })
    }

    /// Resolve which header index maps to each field given the DAT header row.
    ///
    /// Headers that appear in the map but are missing from the DAT are collected.
    /// Core required fields for default map: at least one join key should exist;
    /// we require CONTROL_NUMBER or SHA256 or MESSAGE_ID or ITEM_ID present if
    /// those are in the map.
    pub fn resolve_indices(&self, headers: &[String]) -> Result<HashMap<MappedField, usize>> {
        let header_index: HashMap<String, usize> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| (h.clone(), i))
            .collect();
        // Also allow case-insensitive match.
        let header_ci: HashMap<String, usize> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| (h.to_ascii_uppercase(), i))
            .collect();

        let mut out = HashMap::new();
        let mut missing = Vec::new();
        for (header, field) in &self.map {
            if let Some(&idx) = header_index
                .get(header)
                .or_else(|| header_ci.get(&header.to_ascii_uppercase()))
            {
                out.insert(*field, idx);
            } else {
                // Only hard-require core join keys that are in the default set.
                if is_core_header(header) {
                    missing.push(header.clone());
                }
            }
        }

        // Core required for 0040 default: CONTROL_NUMBER, SHA256, ITEM_ID present when mapped.
        // Soft-optional: MESSAGE_ID etc.
        let core_missing: Vec<String> = missing.into_iter().filter(|h| is_core_header(h)).collect();
        if !core_missing.is_empty() {
            return Err(GapError::InvalidDatHeader {
                missing: core_missing.join(", "),
            });
        }

        // Must have at least one join-capable field resolved.
        let has_join = out.contains_key(&MappedField::ControlNumber)
            || out.contains_key(&MappedField::Sha256)
            || out.contains_key(&MappedField::MessageId)
            || out.contains_key(&MappedField::ItemId)
            || out.contains_key(&MappedField::LogicalHash);
        if !has_join {
            return Err(GapError::InvalidDatHeader {
                missing: "no join key columns (need CONTROL_NUMBER, SHA256, MESSAGE_ID, ITEM_ID, or LOGICAL_HASH)"
                    .into(),
            });
        }

        Ok(out)
    }
}

fn is_core_header(h: &str) -> bool {
    matches!(
        h.to_ascii_uppercase().as_str(),
        "CONTROL_NUMBER" | "SHA256" | "ITEM_ID" | "CUSTODIAN" | "FILE_NAME"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_field_errors() {
        let mut m = HashMap::new();
        m.insert("FOO".into(), "not_a_field".into());
        let err = DatColumnMap::from_string_map(&m).unwrap_err();
        assert!(matches!(err, GapError::InvalidColumnMap(_)));
    }

    #[test]
    fn default_resolve_requires_headers() {
        let map = DatColumnMap::default_produce_v1();
        let headers = vec![
            "CONTROL_NUMBER".into(),
            "SHA256".into(),
            "ITEM_ID".into(),
            "CUSTODIAN".into(),
            "FILE_NAME".into(),
        ];
        let idx = map.resolve_indices(&headers).unwrap();
        assert!(idx.contains_key(&MappedField::ControlNumber));
    }

    #[test]
    fn missing_core_header_errors() {
        let map = DatColumnMap::default_produce_v1();
        let headers = vec!["SUBJECT".into()];
        let err = map.resolve_indices(&headers).unwrap_err();
        assert!(matches!(err, GapError::InvalidDatHeader { .. }));
    }
}
