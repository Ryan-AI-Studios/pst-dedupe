//! Expected-doc ↔ matter set compare with email-aware join order.

use matter_core::{normalize_message_id, GapExpectedDoc, Matter};

use crate::error::Result;

/// How a match was made (for thin matched.csv).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchKey {
    MessageId,
    ItemId,
    LogicalHash,
    NativeSha256,
    ControlNumber,
}

/// One compare outcome for an expected doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareHit {
    pub expected_id: String,
    pub matter_item_id: String,
    pub key: MatchKey,
}

/// Result of comparing one expected set against the matter.
#[derive(Debug, Clone, Default)]
pub struct CompareResult {
    pub matched: Vec<CompareHit>,
    pub unmatched_expected: Vec<GapExpectedDoc>,
    /// Matter item ids with no hit in expected (only when flag_matter_not_in_expected).
    pub unmatched_matter: Vec<String>,
}

/// True when the expected row looks like email.
pub fn is_email_like(doc: &GapExpectedDoc) -> bool {
    if doc
        .message_id
        .as_ref()
        .map(|m| !m.trim().is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    let cat = doc
        .file_category
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    if cat.contains("email") || cat == "mail" || cat == "message" {
        return true;
    }
    let mime = doc.mime_type.as_deref().unwrap_or("").to_ascii_lowercase();
    if mime.contains("message/rfc822")
        || mime.contains("message/rfc2822")
        || mime.contains("outlook")
    {
        return true;
    }
    let ext = doc.file_ext.as_deref().unwrap_or("").to_ascii_lowercase();
    let ext = ext.trim_start_matches('.');
    matches!(ext, "eml" | "msg" | "emlx")
}

/// Join one expected row to matter using the locked key order.
///
/// ```text
/// if has_message_id: try MID (empty MID never matches)
/// if email-like and no mid hit: try item_id / logical_hash
/// if has_sha256: try native_sha256
/// if has_control: try production control_number
/// ```
///
/// When `mid_index` is provided (bulk compare), Message-ID joins are O(1) lookups.
/// When `None`, falls back to [`Matter::find_item_id_by_message_id`].
pub fn match_expected_to_matter(
    matter: &Matter,
    doc: &GapExpectedDoc,
) -> Result<Option<(String, MatchKey)>> {
    match_expected_to_matter_with_mid_index(matter, doc, None)
}

/// Same as [`match_expected_to_matter`] with an optional prebuilt Message-ID index.
pub fn match_expected_to_matter_with_mid_index(
    matter: &Matter,
    doc: &GapExpectedDoc,
    mid_index: Option<&std::collections::HashMap<String, String>>,
) -> Result<Option<(String, MatchKey)>> {
    // 1a Message-ID
    if let Some(ref mid) = doc.message_id {
        let norm = normalize_message_id(mid);
        if !norm.is_empty() {
            let hit = if let Some(index) = mid_index {
                index.get(&norm).cloned()
            } else {
                matter.find_item_id_by_message_id(&norm)?
            };
            if let Some(id) = hit {
                return Ok(Some((id, MatchKey::MessageId)));
            }
        }
    }

    // 1b email-like: item_id / logical_hash
    if is_email_like(doc) {
        if let Some(ref iid) = doc.item_id {
            if matter.find_item_id_exists(iid)? {
                return Ok(Some((iid.clone(), MatchKey::ItemId)));
            }
        }
        if let Some(ref lh) = doc.logical_hash {
            if let Some(id) = matter.find_item_id_by_logical_hash(lh)? {
                return Ok(Some((id, MatchKey::LogicalHash)));
            }
        }
    }

    // 2 SHA-256 (primary for non-email; also for email when MID missing)
    if let Some(ref sha) = doc.sha256 {
        if let Some(id) = matter.find_item_id_by_native_sha256(sha)? {
            return Ok(Some((id, MatchKey::NativeSha256)));
        }
    }

    // Non-email item_id / logical as residual before control
    if !is_email_like(doc) {
        if let Some(ref iid) = doc.item_id {
            if matter.find_item_id_exists(iid)? {
                return Ok(Some((iid.clone(), MatchKey::ItemId)));
            }
        }
        if let Some(ref lh) = doc.logical_hash {
            if let Some(id) = matter.find_item_id_by_logical_hash(lh)? {
                return Ok(Some((id, MatchKey::LogicalHash)));
            }
        }
    }

    // 3 Control number
    if let Some(ref cn) = doc.control_number {
        if let Some(id) = matter.find_item_id_by_control_number(cn)? {
            return Ok(Some((id, MatchKey::ControlNumber)));
        }
    }

    Ok(None)
}

/// Compare all expected docs for an import.
///
/// Builds a one-shot Message-ID index for O(n+m) join cost on the MID path.
pub fn compare_import(
    matter: &Matter,
    docs: &[GapExpectedDoc],
    matter_item_ids: &[String],
    flag_matter_not_in_expected: bool,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<CompareResult> {
    let mut result = CompareResult::default();
    let mut matched_matter: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mid_index = matter.message_id_index()?;

    for (i, doc) in docs.iter().enumerate() {
        if cancel.map(|c| c()).unwrap_or(false) {
            return Err(crate::error::GapError::Cancelled);
        }
        match match_expected_to_matter_with_mid_index(matter, doc, Some(&mid_index))? {
            Some((item_id, key)) => {
                matched_matter.insert(item_id.clone());
                result.matched.push(CompareHit {
                    expected_id: doc.id.clone(),
                    matter_item_id: item_id,
                    key,
                });
            }
            None => result.unmatched_expected.push(doc.clone()),
        }
        progress((i as u64) + 1);
    }

    if flag_matter_not_in_expected {
        for id in matter_item_ids {
            if !matched_matter.contains(id) {
                result.unmatched_matter.push(id.clone());
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_mid_not_email_by_mid_alone() {
        let doc = GapExpectedDoc {
            message_id: Some("  ".into()),
            file_category: Some("document".into()),
            ..Default::default()
        };
        assert!(!is_email_like(&doc));
    }

    #[test]
    fn eml_ext_is_email() {
        let doc = GapExpectedDoc {
            file_ext: Some("eml".into()),
            ..Default::default()
        };
        assert!(is_email_like(&doc));
    }
}
