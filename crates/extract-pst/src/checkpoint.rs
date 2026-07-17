//! Mid-folder extract checkpoints.

use serde::{Deserialize, Serialize};

/// Cursor persisted in `jobs` checkpoint `cursor_json` for stage `pst_extract`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractCursor {
    pub source_id: String,
    pub pst_path: String,
    pub pst_item_id: String,
    #[serde(default)]
    pub pst_native_sha256: Option<String>,
    /// Exact filesystem path used for open when known (e.g. `extract_pst_path`).
    /// Persisted so resume does not re-derive a different file via source+leaf.
    #[serde(default)]
    pub open_fs_path: Option<String>,
    /// Last completed folder path (or current folder when mid-folder).
    #[serde(default)]
    pub last_folder_path: Option<String>,
    /// Last completed message NID as lowercase hex (without 0x).
    #[serde(default)]
    pub last_message_nid: Option<String>,
    /// Index within the current folder's `message_nids` of the last completed message.
    #[serde(default)]
    pub folder_message_index: Option<i64>,
    pub completed_count: u64,
    pub batch_size: u64,
    #[serde(default)]
    pub messages_ok: u64,
    #[serde(default)]
    pub messages_err: u64,
    #[serde(default)]
    pub attachments_ok: u64,
    #[serde(default)]
    pub attachments_err: u64,
}

impl ExtractCursor {
    pub fn new(
        source_id: &str,
        pst_path: &str,
        pst_item_id: &str,
        pst_native_sha256: Option<&str>,
        batch_size: u64,
    ) -> Self {
        Self {
            source_id: source_id.to_string(),
            pst_path: pst_path.to_string(),
            pst_item_id: pst_item_id.to_string(),
            pst_native_sha256: pst_native_sha256.map(|s| s.to_string()),
            open_fs_path: None,
            last_folder_path: None,
            last_message_nid: None,
            folder_message_index: None,
            completed_count: 0,
            batch_size,
            messages_ok: 0,
            messages_err: 0,
            attachments_ok: 0,
            attachments_err: 0,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Format message NID as lowercase hex (no 0x prefix).
pub fn nid_hex(nid: u64) -> String {
    format!("{nid:x}")
}

/// Parse hex NID (with optional 0x).
pub fn parse_nid_hex(s: &str) -> Option<u64> {
    let t = s.trim();
    let t = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    u64::from_str_radix(t, 16).ok()
}
