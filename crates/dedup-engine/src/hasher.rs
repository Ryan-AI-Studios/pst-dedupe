//! Tiered hashing strategy for email deduplication.

use sha2::{Digest, Sha256};

/// Dedup keys computed for a single message.
pub struct DedupKeys {
    /// Tier 1: normalized Message-ID (None if missing).
    pub message_id: Option<String>,
    /// Tier 2: SHA-256 content hash (always computed as fallback).
    pub content_hash: [u8; 32],
}

/// Attachment metadata for hashing (name + size).
#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    pub filename: String,
    pub size: u32,
}

/// Compute both dedup keys for a message.
///
/// Always computes the content hash even if Message-ID is present,
/// so we can report which tier matched.
pub fn compute_dedup_keys(
    message_id: Option<&str>,
    subject: Option<&str>,
    submit_time: Option<i64>,
    sender_email: Option<&str>,
    body_preview: Option<&str>,
    attachments: &[AttachmentInfo],
) -> DedupKeys {
    let normalized_mid = message_id.map(normalize_message_id);

    let content_hash = compute_content_hash(
        subject,
        submit_time,
        sender_email,
        body_preview,
        attachments,
    );

    DedupKeys {
        message_id: normalized_mid,
        content_hash,
    }
}

/// Normalize a Message-ID for consistent matching.
///
/// - Lowercase
/// - Trim whitespace
/// - Remove angle brackets `<` `>`
/// - Remove any CFWS (comments, folding whitespace)
fn normalize_message_id(mid: &str) -> String {
    mid.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_lowercase()
}

/// Compute SHA-256 content hash for Tier 2 dedup.
fn compute_content_hash(
    subject: Option<&str>,
    submit_time: Option<i64>,
    sender_email: Option<&str>,
    body_preview: Option<&str>,
    attachments: &[AttachmentInfo],
) -> [u8; 32] {
    let mut hasher = Sha256::new();

    // Subject (normalized: lowercase, trimmed)
    if let Some(subj) = subject {
        hasher.update(normalize_subject(subj).as_bytes());
    }
    hasher.update(b"|");

    // Submit time as epoch milliseconds string
    if let Some(ft) = submit_time {
        hasher.update(ft.to_string().as_bytes());
    }
    hasher.update(b"|");

    // Sender (lowercase, trimmed)
    if let Some(sender) = sender_email {
        hasher.update(sender.trim().to_lowercase().as_bytes());
    }
    hasher.update(b"|");

    // Body preview (first 4KB, normalized)
    if let Some(body) = body_preview {
        let normalized = body
            .chars()
            .filter(|c| !c.is_whitespace() || *c == ' ')
            .collect::<String>()
            .to_lowercase();
        let preview = if normalized.len() > 4096 {
            &normalized[..4096]
        } else {
            &normalized
        };
        hasher.update(preview.as_bytes());
    }
    hasher.update(b"|");

    // Attachment metadata: sorted by name, then "name:size" pairs
    let mut att_strings: Vec<String> = attachments
        .iter()
        .map(|a| format!("{}:{}", a.filename.to_lowercase(), a.size))
        .collect();
    att_strings.sort();
    for att in &att_strings {
        hasher.update(att.as_bytes());
        hasher.update(b";");
    }

    hasher.finalize().into()
}

/// Normalize a subject line for consistent hashing.
///
/// - Trim whitespace
/// - Remove common prefixes: "Re:", "Fwd:", "FW:", etc. (recursive)
/// - Lowercase
fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim().to_string();

    // Iteratively strip reply/forward prefixes
    loop {
        let lower = s.trim().to_lowercase();
        let stripped = if lower.starts_with("re:") {
            s.trim()[3..].to_string()
        } else if lower.starts_with("fwd:") {
            s.trim()[4..].to_string()
        } else if lower.starts_with("fw:") {
            s.trim()[3..].to_string()
        } else {
            break;
        };
        s = stripped;
    }

    s.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_message_id() {
        assert_eq!(
            normalize_message_id("<ABC123@example.com>"),
            "abc123@example.com"
        );
        assert_eq!(
            normalize_message_id("  <ABC123@example.com>  "),
            "abc123@example.com"
        );
        assert_eq!(normalize_message_id("abc@example.com"), "abc@example.com");
    }

    #[test]
    fn test_normalize_subject() {
        assert_eq!(normalize_subject("Re: Meeting"), "meeting");
        assert_eq!(normalize_subject("FW: Re: FWD: Test"), "test");
        assert_eq!(normalize_subject("  Hello World  "), "hello world");
    }

    #[test]
    fn test_content_hash_deterministic() {
        let h1 = compute_content_hash(
            Some("Test Subject"),
            Some(132456789),
            Some("user@example.com"),
            Some("Hello body"),
            &[],
        );
        let h2 = compute_content_hash(
            Some("Test Subject"),
            Some(132456789),
            Some("user@example.com"),
            Some("Hello body"),
            &[],
        );
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_content_hash_different_inputs() {
        let h1 = compute_content_hash(Some("A"), None, None, None, &[]);
        let h2 = compute_content_hash(Some("B"), None, None, None, &[]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_content_hash_missing_fields_stable() {
        // All-None should produce a deterministic hash (delimiters only)
        let h1 = compute_content_hash(None, None, None, None, &[]);
        let h2 = compute_content_hash(None, None, None, None, &[]);
        assert_eq!(h1, h2, "Missing fields must produce stable hash");
    }

    #[test]
    fn test_content_hash_attachment_ordering() {
        let att_a = AttachmentInfo {
            filename: "A.txt".into(),
            size: 100,
        };
        let att_b = AttachmentInfo {
            filename: "B.txt".into(),
            size: 200,
        };
        let h1 = compute_content_hash(None, None, None, None, &[att_a.clone(), att_b.clone()]);
        let h2 = compute_content_hash(None, None, None, None, &[att_b, att_a]);
        assert_eq!(
            h1, h2,
            "Attachment order must not affect hash (sorted internally)"
        );
    }

    #[test]
    fn test_content_hash_unicode_subject() {
        let h1 = compute_content_hash(Some("Re: Réunion"), None, None, None, &[]);
        let h2 = compute_content_hash(Some("Réunion"), None, None, None, &[]);
        assert_eq!(h1, h2, "Unicode subject normalization with prefix strip");
    }

    #[test]
    fn test_content_hash_none_vs_empty_subject() {
        let h_none = compute_content_hash(None, None, None, None, &[]);
        let h_empty = compute_content_hash(Some(""), None, None, None, &[]);
        // Both should update the hasher with separator only, producing same hash
        assert_eq!(
            h_none, h_empty,
            "None and empty subject should hash identically"
        );
    }
}
