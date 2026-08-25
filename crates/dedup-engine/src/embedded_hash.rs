//! Embedded-message identity hash (`embedded-msg-hash/v1`) — track 0090.
//!
//! **Not Relativity parity.** Recursive hash-in-parent is a pst-dedup product
//! choice for parent-centric keep-sets. Relativity extracts embedded emails as
//! child documents and does not fold nested email into parent AttachmentHash.

use sha2::{Digest, Sha256};

use crate::grouping::{
    normalize_recipient_identity_keys, normalize_recipients, CanonicalRecipient,
};
use crate::hasher::normalize_subject;

/// Domain tag for nested email identity digests (0090).
pub const EMBEDDED_MSG_HASH_DOMAIN: &[u8] = b"pst-dedup/embedded-msg-hash/v1\0";

/// Domain tag when nested recursion hits the depth cap (0090).
pub const ATTACH_DEPTH_LIMIT_DOMAIN: &[u8] = b"pst-dedup/attach-depth-limit/v1\0";

/// Domain tag when nested body is truly missing/unreadable (not empty body).
pub const EMBEDDED_BODY_MISSING_DOMAIN: &[u8] = b"pst-dedup/embedded-body-missing/v1\0";

/// Max nested embed depth for identity parse (align D-0067 honesty).
/// Depth `0` = outermost embed under the parent; at `depth >=` this value use
/// [`attach_depth_limit_sentinel`] instead of further parse.
pub const MAX_EMBEDDED_MSG_DEPTH: u8 = 3;

/// Sentinel body component when nested `PidTagBody` is absent/unreadable.
///
/// Distinct from [`crate::hasher::hash_full_body`] over `""` (legitimate empty body).
pub fn embedded_body_missing_hash() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(EMBEDDED_BODY_MISSING_DOMAIN);
    hasher.finalize().into()
}

/// Depth-limit sentinel for an attach slot that would recurse past the cap.
///
/// Exact preimage:
/// `SHA-256( b"pst-dedup/attach-depth-limit/v1\0" || name_lower || b"\0" || size_le_u32 )`
pub fn attach_depth_limit_sentinel(filename: &str, size: u32) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(ATTACH_DEPTH_LIMIT_DOMAIN);
    hasher.update(filename.to_lowercase().as_bytes());
    hasher.update(b"\0");
    hasher.update(size.to_le_bytes());
    hasher.finalize().into()
}

/// Header component: `SHA-256(norm_subject || "|" || submit_time_str || "|" || sender_lower)`.
///
/// Submit time is decimal string when `Some` (same as outer hasher); absent → empty.
/// Sender is `trim().to_lowercase()` when present.
pub fn embedded_header_hash(
    subject: Option<&str>,
    submit_time: Option<i64>,
    sender: Option<&str>,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    if let Some(subj) = subject {
        hasher.update(normalize_subject(subj).as_bytes());
    }
    hasher.update(b"|");
    if let Some(ft) = submit_time {
        hasher.update(ft.to_string().as_bytes());
    }
    hasher.update(b"|");
    if let Some(s) = sender {
        hasher.update(s.trim().to_lowercase().as_bytes());
    }
    hasher.finalize().into()
}

/// Recipients component: `SHA-256(recipient_strong_preimage bytes)`.
///
/// Structured identity keys when `recipients` is non-empty and yields keys;
/// else display To|Cc|Bcc path (same rules as Tier-2.5).
pub fn embedded_recipients_hash(
    display_to: Option<&str>,
    display_cc: Option<&str>,
    display_bcc: Option<&str>,
    recipients: Option<&[CanonicalRecipient]>,
) -> [u8; 32] {
    let preimage = recipient_strong_preimage_bytes(display_to, display_cc, display_bcc, recipients);
    let mut hasher = Sha256::new();
    hasher.update(preimage.as_bytes());
    hasher.finalize().into()
}

fn recipient_strong_preimage_bytes(
    display_to: Option<&str>,
    display_cc: Option<&str>,
    display_bcc: Option<&str>,
    recipients: Option<&[CanonicalRecipient]>,
) -> String {
    if let Some(rows) = recipients {
        if !rows.is_empty() {
            let keys: Vec<String> = rows.iter().filter_map(|r| r.identity_key()).collect();
            if !keys.is_empty() {
                return normalize_recipient_identity_keys(keys);
            }
        }
    }
    format!(
        "{}|{}|{}",
        normalize_recipients(display_to.unwrap_or("")),
        normalize_recipients(display_cc.unwrap_or("")),
        normalize_recipients(display_bcc.unwrap_or(""))
    )
}

/// Attachments component: `SHA-256(digest0 || ";" || digest1 || ";" || …)` in
/// **attachment table index order** (not sorted).
pub fn embedded_attachments_hash(child_digests_in_index_order: &[[u8; 32]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for d in child_digests_in_index_order {
        hasher.update(d);
        hasher.update(b";");
    }
    hasher.finalize().into()
}

/// Full `embedded-msg-hash/v1` digest for one nested message.
pub fn compute_embedded_msg_hash_v1(
    depth: u8,
    header_hash: [u8; 32],
    body_hash: [u8; 32],
    recipients_hash: [u8; 32],
    attachments_hash: [u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(EMBEDDED_MSG_HASH_DOMAIN);
    hasher.update([depth]);
    hasher.update(header_hash);
    hasher.update(body_hash);
    hasher.update(recipients_hash);
    hasher.update(attachments_hash);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grouping::{CanonicalRecipient, CanonicalRecipientType};
    use crate::hasher::{
        attach_unread_sentinel, hash_full_body, ATTACH_UNREAD_DOMAIN, EMPTY_CONTENT_SHA256,
    };

    #[test]
    fn domain_separation_depth_unread_empty() {
        let depth = attach_depth_limit_sentinel("nested.msg", 100);
        let unread = attach_unread_sentinel("nested.msg", 100);
        assert_ne!(depth, unread);
        assert_ne!(depth, EMPTY_CONTENT_SHA256);
        assert_ne!(unread, EMPTY_CONTENT_SHA256);
        // Same name/size must still domain-separate from unread domain constant path.
        assert_ne!(ATTACH_DEPTH_LIMIT_DOMAIN, ATTACH_UNREAD_DOMAIN);
    }

    #[test]
    fn header_change_changes_embedded_hash() {
        let (body, _) = hash_full_body("same body");
        let recip = embedded_recipients_hash(Some("a@x.com"), None, None, None);
        let atts = embedded_attachments_hash(&[]);
        let h1 = embedded_header_hash(Some("Subject A"), Some(100), Some("alice@ex.com"));
        let h2 = embedded_header_hash(Some("Subject B"), Some(100), Some("alice@ex.com"));
        let d1 = compute_embedded_msg_hash_v1(0, h1, body, recip, atts);
        let d2 = compute_embedded_msg_hash_v1(0, h2, body, recip, atts);
        assert_ne!(d1, d2, "subject must participate in embedded-msg-hash/v1");
    }

    #[test]
    fn body_change_changes_embedded_hash() {
        let header = embedded_header_hash(Some("S"), Some(1), Some("a@x.com"));
        let recip = embedded_recipients_hash(None, None, None, None);
        let atts = embedded_attachments_hash(&[]);
        let (b1, _) = hash_full_body("body one");
        let (b2, _) = hash_full_body("body two");
        let d1 = compute_embedded_msg_hash_v1(0, header, b1, recip, atts);
        let d2 = compute_embedded_msg_hash_v1(0, header, b2, recip, atts);
        assert_ne!(d1, d2);
    }

    #[test]
    fn depth_byte_participates() {
        let header = embedded_header_hash(Some("S"), None, None);
        let (body, _) = hash_full_body("");
        let recip = embedded_recipients_hash(None, None, None, None);
        let atts = embedded_attachments_hash(&[]);
        let d0 = compute_embedded_msg_hash_v1(0, header, body, recip, atts);
        let d1 = compute_embedded_msg_hash_v1(1, header, body, recip, atts);
        assert_ne!(d0, d1);
    }

    #[test]
    fn attachments_hash_index_order_not_sorted() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let ab = embedded_attachments_hash(&[a, b]);
        let ba = embedded_attachments_hash(&[b, a]);
        assert_ne!(ab, ba, "child attach digests must stay in index order");
    }

    #[test]
    fn structured_recipients_affect_hash() {
        let display = embedded_recipients_hash(Some("Alice"), None, None, None);
        let rows = [CanonicalRecipient {
            recipient_type: CanonicalRecipientType::To,
            display_name: Some("Alice".into()),
            address_type: Some("SMTP".into()),
            email_address: Some("alice@ex.com".into()),
            smtp_address: Some("alice@ex.com".into()),
        }];
        let structured = embedded_recipients_hash(Some("Alice"), None, None, Some(&rows));
        assert_ne!(display, structured);
    }

    #[test]
    fn max_depth_constant() {
        assert_eq!(MAX_EMBEDDED_MSG_DEPTH, 3);
    }

    #[test]
    fn missing_body_hash_ne_empty_body_hash() {
        let missing = embedded_body_missing_hash();
        let (empty, _) = hash_full_body("");
        assert_ne!(
            missing, empty,
            "absent nested body must not collide with empty-body hash_full_body"
        );
        assert_ne!(missing, EMPTY_CONTENT_SHA256);
        assert_ne!(EMBEDDED_BODY_MISSING_DOMAIN, EMBEDDED_MSG_HASH_DOMAIN);
        assert_ne!(EMBEDDED_BODY_MISSING_DOMAIN, ATTACH_DEPTH_LIMIT_DOMAIN);
        assert_ne!(EMBEDDED_BODY_MISSING_DOMAIN, ATTACH_UNREAD_DOMAIN);
    }
}
