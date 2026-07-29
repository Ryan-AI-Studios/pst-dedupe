//! Tiered hashing strategy for email deduplication (0076 hardened).
//!
//! Tier 1: normalized Message-ID.
//! Tier 2 (v1): subject | submit | sender | ≤4096 **chars** body preview | attach name:size.
//! Tier 2.5 (v2): v1 preimage bytes unchanged, then optional full-body / recipients / attach content.

use sha2::{Digest, Sha256};

use crate::grouping::{
    normalize_recipient_identity_keys, normalize_recipients, CanonicalRecipient, IdentityLevel,
};

/// Dedup keys computed for a single message.
#[derive(Debug, Clone)]
pub struct DedupKeys {
    /// Tier 1: normalized Message-ID (None if missing).
    pub message_id: Option<String>,
    /// Tier 2 v1 content hash (always computed; hex stable except char-clamp exception).
    pub content_hash: [u8; 32],
    /// True when normalized body preview exceeded 4096 **bytes** (hash may differ from pre-0076).
    pub preview_bytes_over_budget: bool,
    /// Component fingerprints (attribution only; never bind).
    pub fp_header: u64,
    pub fp_body: u64,
    pub fp_recipients: u64,
    pub fp_attachments: u64,
    /// Strong (v2) content hash when identity level ≥ body; else None.
    pub strong_content_hash: Option<[u8; 32]>,
}

/// Attachment metadata for hashing (name + size + optional inline/content signals).
#[derive(Debug, Clone, Default)]
pub struct AttachmentInfo {
    pub filename: String,
    pub size: u32,
    /// True when MAPI marks this as inline/embedded (content-id / rendered-in-body / hidden).
    pub is_inline: bool,
    /// Content digest slot for `--strong-content-hash body-recip-attach`.
    ///
    /// When the identity level includes attach content, every non-ignored attach
    /// must contribute a 32-byte slot: real `SHA-256(stream_bytes)` or a Choice B
    /// unread sentinel ([`attach_unread_sentinel`]). Prefer filling this before
    /// strong-hash construction; [`compute_strong_content_hash`] also synthesizes
    /// the sentinel when `None` so slots are never omitted.
    pub content_sha256: Option<[u8; 32]>,
}

impl AttachmentInfo {
    pub fn new(filename: impl Into<String>, size: u32) -> Self {
        Self {
            filename: filename.into(),
            size,
            is_inline: false,
            content_sha256: None,
        }
    }
}

/// Domain tag for Choice B unread attach-content sentinels (0086).
///
/// Exact preimage:
/// `SHA-256( b"pst-dedup/attach-unread/v1\0" || name_lower_utf8 || b"\0" || size_le_u32 )`
pub const ATTACH_UNREAD_DOMAIN: &[u8] = b"pst-dedup/attach-unread/v1\0";

/// SHA-256 of the empty string (legitimate zero-byte by-value attach).
pub const EMPTY_CONTENT_SHA256: [u8; 32] = [
    0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
    0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
];

/// Choice B domain-separated unread sentinel for one attachment slot.
///
/// Incorporates normalized filename + declared size so unread `Contract.pdf` ≠
/// unread `Financials.xlsx` ≠ empty-file digest ≠ real content digests.
pub fn attach_unread_sentinel(filename: &str, size: u32) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(ATTACH_UNREAD_DOMAIN);
    hasher.update(filename.to_lowercase().as_bytes());
    hasher.update(b"\0");
    hasher.update(size.to_le_bytes());
    hasher.finalize().into()
}

/// Resolve the 32-byte attach-content slot for strong identity (Choice B).
///
/// - When `content_sha256` is `Some`, use it (real digest or pre-filled sentinel).
/// - When `None` (or `is_unread`), produce [`attach_unread_sentinel`].
///
/// Callers that digest streams should pass `Some(real)` on success and either
/// `Some(sentinel)` or `None` on failure — both yield a non-omitted slot.
pub fn attach_content_slot(
    filename: &str,
    size: u32,
    content_sha256: Option<[u8; 32]>,
    is_unread: bool,
) -> [u8; 32] {
    if is_unread {
        return attach_unread_sentinel(filename, size);
    }
    content_sha256.unwrap_or_else(|| attach_unread_sentinel(filename, size))
}

/// Inputs for content-hash / strong-hash computation beyond the classic v1 fields.
#[derive(Debug, Clone, Default)]
pub struct StrongHashInput<'a> {
    pub identity: IdentityLevel,
    /// Full normalized body digest (SHA-256 over full normalized body text).
    pub body_sha256: Option<&'a [u8; 32]>,
    pub body_char_len: Option<u64>,
    pub display_to: Option<&'a str>,
    pub display_cc: Option<&'a str>,
    pub display_bcc: Option<&'a str>,
    /// Structured recipient TC rows (0082). When non-empty, Tier-2.5 uses each
    /// row's identity key (SMTP → EX DN → display) over To+Cc+Bcc instead of
    /// display strings. Table-less messages leave this empty/None and keep the
    /// display-string path.
    pub recipients: Option<&'a [CanonicalRecipient]>,
    /// When true, inline attachments are omitted from the attachment component.
    pub ignore_inline_attachments: bool,
}

/// Compute both dedup keys for a message (v1 content hash; optional strong).
///
/// Always computes the v1 content hash even if Message-ID is present.
pub fn compute_dedup_keys(
    message_id: Option<&str>,
    subject: Option<&str>,
    submit_time: Option<i64>,
    sender_email: Option<&str>,
    body_preview: Option<&str>,
    attachments: &[AttachmentInfo],
) -> DedupKeys {
    compute_dedup_keys_ex(
        message_id,
        subject,
        submit_time,
        sender_email,
        body_preview,
        attachments,
        &StrongHashInput::default(),
    )
}

/// Extended key computation with Tier-2.5 inputs.
pub fn compute_dedup_keys_ex(
    message_id: Option<&str>,
    subject: Option<&str>,
    submit_time: Option<i64>,
    sender_email: Option<&str>,
    body_preview: Option<&str>,
    attachments: &[AttachmentInfo],
    strong: &StrongHashInput<'_>,
) -> DedupKeys {
    let normalized_mid = message_id.map(normalize_message_id);

    let (content_hash, preview_over, fps) = compute_content_hash_detailed(
        subject,
        submit_time,
        sender_email,
        body_preview,
        attachments,
        strong.ignore_inline_attachments,
    );

    let fp_recipients = recipients_fingerprint_ex(
        strong.display_to,
        strong.display_cc,
        strong.display_bcc,
        strong.recipients,
    );

    let strong_content_hash = if strong.identity.is_strong() {
        Some(compute_strong_content_hash(
            subject,
            submit_time,
            sender_email,
            body_preview,
            attachments,
            strong,
        ))
    } else {
        None
    };

    DedupKeys {
        message_id: normalized_mid,
        content_hash,
        preview_bytes_over_budget: preview_over,
        fp_header: fps.0,
        fp_body: fps.1,
        fp_recipients,
        fp_attachments: fps.3,
        strong_content_hash,
    }
}

/// Normalize a Message-ID for consistent matching.
///
/// - Lowercase
/// - Trim whitespace
/// - Remove angle brackets `<` `>`
pub fn normalize_message_id(mid: &str) -> String {
    mid.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_lowercase()
}

/// Public Tier-2 v1 content hash (stable API of `dedup-engine`).
///
/// Body preview is clamped to 4096 **characters** (not bytes).
pub fn compute_content_hash(
    subject: Option<&str>,
    submit_time: Option<i64>,
    sender_email: Option<&str>,
    body_preview: Option<&str>,
    attachments: &[AttachmentInfo],
) -> [u8; 32] {
    compute_content_hash_detailed(
        subject,
        submit_time,
        sender_email,
        body_preview,
        attachments,
        false,
    )
    .0
}

/// Like [`compute_content_hash`] plus over-budget flag and component fingerprints.
pub fn compute_content_hash_detailed(
    subject: Option<&str>,
    submit_time: Option<i64>,
    sender_email: Option<&str>,
    body_preview: Option<&str>,
    attachments: &[AttachmentInfo],
    ignore_inline: bool,
) -> ([u8; 32], bool, (u64, u64, u64, u64)) {
    let mut hasher = Sha256::new();
    let mut header_h = Sha256::new();
    let mut body_h = Sha256::new();
    let mut attach_h = Sha256::new();

    // Subject (normalized)
    if let Some(subj) = subject {
        let n = normalize_subject(subj);
        hasher.update(n.as_bytes());
        header_h.update(n.as_bytes());
    }
    hasher.update(b"|");
    header_h.update(b"|");

    // Submit time
    if let Some(ft) = submit_time {
        let s = ft.to_string();
        hasher.update(s.as_bytes());
        header_h.update(s.as_bytes());
    }
    hasher.update(b"|");
    header_h.update(b"|");

    // Sender
    if let Some(sender) = sender_email {
        let s = sender.trim().to_lowercase();
        hasher.update(s.as_bytes());
        header_h.update(s.as_bytes());
    }
    hasher.update(b"|");
    header_h.update(b"|");

    // Body preview — character clamp (0076 D1)
    let mut preview_over = false;
    if let Some(body) = body_preview {
        let normalized = normalize_body_text(body);
        if normalized.len() > 4096 {
            preview_over = true;
        }
        let preview: String = normalized.chars().take(4096).collect();
        hasher.update(preview.as_bytes());
        body_h.update(preview.as_bytes());
    }
    hasher.update(b"|");

    // Attachment metadata: sorted name:size (optionally skip inline)
    let att_strings = attachment_meta_strings(attachments, ignore_inline);
    for att in &att_strings {
        hasher.update(att.as_bytes());
        hasher.update(b";");
        attach_h.update(att.as_bytes());
        attach_h.update(b";");
    }

    let content_hash: [u8; 32] = hasher.finalize().into();
    let fp_header = fingerprint64(&header_h.finalize());
    let fp_body = fingerprint64(&body_h.finalize());
    let fp_attachments = fingerprint64(&attach_h.finalize());
    // Recipients not in v1 preimage — fingerprint still available when computed later.
    let fp_recipients = 0u64;

    (
        content_hash,
        preview_over,
        (fp_header, fp_body, fp_recipients, fp_attachments),
    )
}

/// Compute recipient-component fingerprint (attribution) from display strings.
pub fn recipients_fingerprint(
    display_to: Option<&str>,
    display_cc: Option<&str>,
    display_bcc: Option<&str>,
) -> u64 {
    recipients_fingerprint_ex(display_to, display_cc, display_bcc, None)
}

/// Recipient fingerprint preferring structured identity keys when present (0082).
///
/// When the table is non-empty but every `identity_key()` is `None`, fall back to
/// the display-string path (never hash an empty structured fingerprint).
pub fn recipients_fingerprint_ex(
    display_to: Option<&str>,
    display_cc: Option<&str>,
    display_bcc: Option<&str>,
    recipients: Option<&[CanonicalRecipient]>,
) -> u64 {
    let mut h = Sha256::new();
    if let Some(rows) = recipients {
        if !rows.is_empty() {
            let keys: Vec<String> = rows.iter().filter_map(|r| r.identity_key()).collect();
            if !keys.is_empty() {
                h.update(normalize_recipient_identity_keys(keys).as_bytes());
                return fingerprint64(&h.finalize());
            }
            // All identity keys empty — fall through to display path.
        }
    }
    h.update(normalize_recipients(display_to.unwrap_or("")).as_bytes());
    h.update(b"|");
    h.update(normalize_recipients(display_cc.unwrap_or("")).as_bytes());
    h.update(b"|");
    h.update(normalize_recipients(display_bcc.unwrap_or("")).as_bytes());
    fingerprint64(&h.finalize())
}

/// Build the Tier-2.5 recipient preimage fragment (display or structured).
///
/// Empty-identity table rows fall back to display strings (same as fingerprint path).
fn recipient_strong_preimage(strong: &StrongHashInput<'_>) -> String {
    if let Some(rows) = strong.recipients {
        if !rows.is_empty() {
            let keys: Vec<String> = rows.iter().filter_map(|r| r.identity_key()).collect();
            if !keys.is_empty() {
                // Single sorted join (To+Cc+Bcc together) — BCC participates in identity.
                return normalize_recipient_identity_keys(keys);
            }
            // All identity keys empty — fall through to display path.
        }
    }
    // Table-less / empty-identity path: three display fields, same as pre-0082.
    format!(
        "{}|{}|{}",
        normalize_recipients(strong.display_to.unwrap_or("")),
        normalize_recipients(strong.display_cc.unwrap_or("")),
        normalize_recipients(strong.display_bcc.unwrap_or(""))
    )
}

/// v2 preimage = v1 preimage bytes, then layered extras. equal-v2 ⇒ equal-v1.
fn compute_strong_content_hash(
    subject: Option<&str>,
    submit_time: Option<i64>,
    sender_email: Option<&str>,
    body_preview: Option<&str>,
    attachments: &[AttachmentInfo],
    strong: &StrongHashInput<'_>,
) -> [u8; 32] {
    let mut hasher = Sha256::new();

    // ── identical v1 preimage ──────────────────────────────────────────────
    if let Some(subj) = subject {
        hasher.update(normalize_subject(subj).as_bytes());
    }
    hasher.update(b"|");
    if let Some(ft) = submit_time {
        hasher.update(ft.to_string().as_bytes());
    }
    hasher.update(b"|");
    if let Some(sender) = sender_email {
        hasher.update(sender.trim().to_lowercase().as_bytes());
    }
    hasher.update(b"|");
    if let Some(body) = body_preview {
        let normalized = normalize_body_text(body);
        let preview: String = normalized.chars().take(4096).collect();
        hasher.update(preview.as_bytes());
    }
    hasher.update(b"|");
    let att_strings = attachment_meta_strings(attachments, strong.ignore_inline_attachments);
    for att in &att_strings {
        hasher.update(att.as_bytes());
        hasher.update(b";");
    }

    // ── v2 extras ──────────────────────────────────────────────────────────
    hasher.update(b"|v2|");
    if strong.identity.includes_body() {
        if let Some(b) = strong.body_sha256 {
            hasher.update(b);
        }
        hasher.update(b"|");
        if let Some(len) = strong.body_char_len {
            hasher.update(len.to_string().as_bytes());
        }
        hasher.update(b"|");
    }
    if strong.identity.includes_recipients() {
        // Structured table: one sorted identity-key join (includes BCC).
        // Display path: three normalized fields joined with '|' (pre-0082).
        let recip = recipient_strong_preimage(strong);
        hasher.update(recip.as_bytes());
        hasher.update(b"|");
    }
    if strong.identity.includes_attach_content() {
        // Choice B (0086): every non-ignored attach contributes exactly one
        // 32-byte slot (real digest or name+size domain-separated unread
        // sentinel). Never omit missing digests; never tier-downgrade.
        let mut digests: Vec<[u8; 32]> = attachments
            .iter()
            .filter(|a| !(strong.ignore_inline_attachments && a.is_inline))
            .map(|a| attach_content_slot(&a.filename, a.size, a.content_sha256, false))
            .collect();
        digests.sort_unstable();
        for d in &digests {
            hasher.update(d);
            hasher.update(b";");
        }
    }

    hasher.finalize().into()
}

fn attachment_meta_strings(attachments: &[AttachmentInfo], ignore_inline: bool) -> Vec<String> {
    let mut att_strings: Vec<String> = attachments
        .iter()
        .filter(|a| !(ignore_inline && a.is_inline))
        .map(|a| format!("{}:{}", a.filename.to_lowercase(), a.size))
        .collect();
    att_strings.sort();
    att_strings
}

fn fingerprint64(digest: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let n = digest.len().min(8);
    buf[..n].copy_from_slice(&digest[..n]);
    u64::from_le_bytes(buf)
}

/// Whitespace normalize: keep spaces, drop other whitespace; lowercase.
pub fn normalize_body_text(body: &str) -> String {
    body.chars()
        .filter(|c| !c.is_whitespace() || *c == ' ')
        .collect::<String>()
        .to_lowercase()
}

/// SHA-256 of full normalized body (for Tier-2.5 body level).
pub fn hash_full_body(body: &str) -> ([u8; 32], u64) {
    let normalized = normalize_body_text(body);
    let char_len = normalized.chars().count() as u64;
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    (hasher.finalize().into(), char_len)
}

/// Normalize a subject line for consistent hashing.
///
/// - Trim whitespace
/// - Remove common prefixes: "Re:", "Fwd:", "FW:" (recursive)
/// - Lowercase
///
/// Prefix strip is ASCII-safe: only slice when `is_char_boundary` holds.
pub fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim().to_string();

    loop {
        let trimmed = s.trim();
        let lower = trimmed.to_lowercase();
        let strip_len = if lower.starts_with("re:") {
            3usize
        } else if lower.starts_with("fwd:") {
            4
        } else if lower.starts_with("fw:") {
            3
        } else {
            break;
        };
        // Guard against mid-char slices (pathological multibyte after lowercase).
        if !trimmed.is_char_boundary(strip_len) {
            break;
        }
        s = trimmed[strip_len..].to_string();
    }

    s.trim().to_lowercase()
}

/// Tier-2 eligibility (0076 §3.3).
///
/// Ineligible when body is known-unreadable OR preimage is degenerate
/// (no body and fewer than 2 weak fields among subject/time/sender/≥1 attach).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier2IneligibleReason {
    UnreadableBody,
    Degenerate,
    /// Kept-despite-CRC taint (0077); Tier-2 blocked unless `--allow-crc-suspect-tier2`.
    CrcSuspect,
}

/// Assess Tier-2 eligibility from integrity + weak-field presence.
///
/// `has_body_preview` means the body property was **successfully read**, including a
/// genuinely empty body (`Some("")`). Clean empty is not degraded and still binds (§3.3).
/// Only absent/unread bodies fall through to the degenerate weak-field check.
pub fn tier2_eligibility(
    body_incomplete: bool,
    body_unavailable: bool,
    has_body_preview: bool,
    subject_nonempty: bool,
    submit_time_present: bool,
    sender_nonempty: bool,
    attach_count: usize,
) -> Result<(), Tier2IneligibleReason> {
    if body_incomplete || body_unavailable {
        return Err(Tier2IneligibleReason::UnreadableBody);
    }
    if has_body_preview {
        return Ok(());
    }
    let mut weak = 0u32;
    if subject_nonempty {
        weak += 1;
    }
    if submit_time_present {
        weak += 1;
    }
    if sender_nonempty {
        weak += 1;
    }
    if attach_count >= 1 {
        weak += 1;
    }
    if weak < 2 {
        return Err(Tier2IneligibleReason::Degenerate);
    }
    Ok(())
}

/// Count weak fields for stats / callers.
pub fn count_weak_fields(
    subject_nonempty: bool,
    submit_time_present: bool,
    sender_nonempty: bool,
    attach_count: usize,
) -> u32 {
    let mut weak = 0u32;
    if subject_nonempty {
        weak += 1;
    }
    if submit_time_present {
        weak += 1;
    }
    if sender_nonempty {
        weak += 1;
    }
    if attach_count >= 1 {
        weak += 1;
    }
    weak
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checked-in pre-0076 digest for pure-ASCII long body (Phase 0 baseline).
    const ASCII_LONG_BODY_V1: &str =
        "a163356e463e6b6bf1c52dd1977779d2bb99546bd63497c0c016317d488fbf5d";

    fn hex(h: &[u8; 32]) -> String {
        h.iter().map(|b| format!("{b:02x}")).collect()
    }

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
        let h1 = compute_content_hash(None, None, None, None, &[]);
        let h2 = compute_content_hash(None, None, None, None, &[]);
        assert_eq!(h1, h2, "Missing fields must produce stable hash");
    }

    #[test]
    fn test_content_hash_attachment_ordering() {
        let att_a = AttachmentInfo::new("A.txt", 100);
        let att_b = AttachmentInfo::new("B.txt", 200);
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
        assert_eq!(
            h_none, h_empty,
            "None and empty subject should hash identically"
        );
    }

    #[test]
    fn cjk_4096_char_no_panic_and_covers_all_chars() {
        let body = "\u{3042}".repeat(4096);
        let (h1, over, _) =
            compute_content_hash_detailed(None, None, None, Some(&body), &[], false);
        // CJK 3-byte chars → 12288 bytes normalized → over budget.
        assert!(over);
        // Differ only at character 3000 — must split (char clamp, not byte clamp).
        let mut chars: Vec<char> = body.chars().collect();
        chars[2999] = 'い';
        let body2: String = chars.into_iter().collect();
        let (h2, _, _) = compute_content_hash_detailed(None, None, None, Some(&body2), &[], false);
        assert_ne!(
            h1, h2,
            "difference at char 3000 must split under char clamp"
        );
    }

    #[test]
    fn ascii_long_body_stable_checked_in_digest() {
        let body = "a".repeat(5000);
        let (h, over, _) = compute_content_hash_detailed(None, None, None, Some(&body), &[], false);
        // Normalized length 5000 > 4096 → over-budget flag; digest is still
        // byte-identical to pre-0076 because first 4096 chars == first 4096 bytes.
        assert!(over);
        assert_eq!(hex(&h), ASCII_LONG_BODY_V1);
    }

    #[test]
    fn cyrillic_char_clamp_differs_and_is_split_only() {
        let body = "я".repeat(4096);
        let (h_char, over, _) =
            compute_content_hash_detailed(None, None, None, Some(&body), &[], false);
        assert!(over);
        // Pre-0076 byte-clamped value (Phase 0 baseline).
        assert_ne!(
            hex(&h_char),
            "78df2ccca7afb7825a3e697b1d8ef6e539e9056bfec067debf3cde3725e5407a"
        );
        assert_eq!(
            hex(&h_char),
            "f88faa1821f7e26e2ef16756f16f13859f7101d168f1ca1483b71d68dcd32cc3"
        );
        // Split-only: pair differing only after char 2048 separates under char clamp.
        let mut chars: Vec<char> = body.chars().collect();
        chars[3000] = 'ю';
        let body2: String = chars.into_iter().collect();
        let (h2, _, _) = compute_content_hash_detailed(None, None, None, Some(&body2), &[], false);
        assert_ne!(h_char, h2);
        // Pair differing before 2048 still separated.
        let mut chars3: Vec<char> = body.chars().collect();
        chars3[100] = 'ю';
        let body3: String = chars3.into_iter().collect();
        let (h3, _, _) = compute_content_hash_detailed(None, None, None, Some(&body3), &[], false);
        assert_ne!(h_char, h3);
    }

    #[test]
    fn pathological_subjects_no_panic() {
        // Multibyte tail after Re:
        let _ = normalize_subject(&format!("Re:{}", "あ".repeat(10)));
        // Multibyte before prefix test path
        let _ = normalize_subject("あRe: test");
        let _ = compute_content_hash(Some("Re:あ"), None, None, None, &[]);
        let _ = compute_content_hash(Some("あRe: x"), None, None, None, &[]);
    }

    #[test]
    fn tier2_eligibility_unreadable() {
        assert_eq!(
            tier2_eligibility(true, false, false, true, true, true, 0),
            Err(Tier2IneligibleReason::UnreadableBody)
        );
        assert_eq!(
            tier2_eligibility(false, true, false, true, true, true, 0),
            Err(Tier2IneligibleReason::UnreadableBody)
        );
    }

    #[test]
    fn tier2_eligibility_degenerate() {
        // No body, only one weak field → degenerate
        assert_eq!(
            tier2_eligibility(false, false, false, true, false, false, 0),
            Err(Tier2IneligibleReason::Degenerate)
        );
        // No body, two weak fields → ok
        assert!(tier2_eligibility(false, false, false, true, true, false, 0).is_ok());
        // Body present (incl. clean empty) → ok even with zero weak fields
        assert!(tier2_eligibility(false, false, true, false, false, false, 0).is_ok());
    }

    #[test]
    fn tier2_eligibility_clean_empty_body_binds() {
        // Successfully-read empty body is present, not unreadable, and still binds.
        assert!(tier2_eligibility(false, false, true, false, false, false, 0).is_ok());
        // Unreadable still blocks even if a stale has_body flag were true.
        assert_eq!(
            tier2_eligibility(false, true, true, true, true, true, 1),
            Err(Tier2IneligibleReason::UnreadableBody)
        );
    }

    #[test]
    fn v2_refinement_property() {
        // Over ≥1000 generated field tuples: equal strong ⇒ equal v1.
        let mut seen_v2: std::collections::HashMap<[u8; 32], [u8; 32]> =
            std::collections::HashMap::new();
        for i in 0..1000u64 {
            let subj = format!("Subject {i}");
            let body = format!(
                "Body text number {i} with padding {}",
                "x".repeat((i % 50) as usize)
            );
            let recip = if i % 3 == 0 {
                Some("a@x.com; b@y.com")
            } else if i % 3 == 1 {
                Some("b@y.com;a@x.com")
            } else {
                Some("solo@z.com")
            };
            let (body_sha, body_len) = hash_full_body(&body);
            let atts = [AttachmentInfo::new(format!("f{i}.txt"), (i % 1000) as u32)];
            let strong = StrongHashInput {
                identity: IdentityLevel::BodyRecip,
                body_sha256: Some(&body_sha),
                body_char_len: Some(body_len),
                display_to: recip,
                display_cc: None,
                display_bcc: if i % 5 == 0 { Some("bcc@x.com") } else { None },
                recipients: None,
                ignore_inline_attachments: false,
            };
            let k = compute_dedup_keys_ex(
                None,
                Some(&subj),
                Some(i as i64),
                Some("s@x.com"),
                Some(&body),
                &atts,
                &strong,
            );
            let v2 = k.strong_content_hash.expect("strong hash");
            if let Some(prev_v1) = seen_v2.insert(v2, k.content_hash) {
                assert_eq!(
                    prev_v1, k.content_hash,
                    "equal-v2 must imply equal-v1 at i={i}"
                );
            }
        }
        // Pair identical in first 4KB chars but divergent after → split at body, not at off.
        let prefix: String = "p".repeat(4096);
        let body_a = format!("{prefix}AAAA");
        let body_b = format!("{prefix}BBBB");
        // Preview is char-clamped to 4096, so v1 hashes equal.
        let h_a = compute_content_hash(None, None, None, Some(&body_a), &[]);
        let h_b = compute_content_hash(None, None, None, Some(&body_b), &[]);
        assert_eq!(h_a, h_b, "v1 preview should match");
        let (sha_a, len_a) = hash_full_body(&body_a);
        let (sha_b, len_b) = hash_full_body(&body_b);
        let s_a = StrongHashInput {
            identity: IdentityLevel::Body,
            body_sha256: Some(&sha_a),
            body_char_len: Some(len_a),
            ..Default::default()
        };
        let s_b = StrongHashInput {
            identity: IdentityLevel::Body,
            body_sha256: Some(&sha_b),
            body_char_len: Some(len_b),
            ..Default::default()
        };
        let k_a = compute_dedup_keys_ex(None, None, None, None, Some(&body_a), &[], &s_a);
        let k_b = compute_dedup_keys_ex(None, None, None, None, Some(&body_b), &[], &s_b);
        assert_ne!(
            k_a.strong_content_hash, k_b.strong_content_hash,
            "strong body level must split after-preview divergence"
        );
    }

    #[test]
    fn recipient_normalization_in_strong_hash() {
        let (sha, len) = hash_full_body("hello");
        let s1 = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("A@x.com; b@X.com"),
            display_cc: None,
            display_bcc: None,
            ..Default::default()
        };
        let s2 = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("b@x.com;a@x.com"),
            display_cc: None,
            display_bcc: None,
            ..Default::default()
        };
        let k1 = compute_dedup_keys_ex(None, Some("S"), None, None, Some("hello"), &[], &s1);
        let k2 = compute_dedup_keys_ex(None, Some("S"), None, None, Some("hello"), &[], &s2);
        assert_eq!(k1.strong_content_hash, k2.strong_content_hash);
    }

    /// Typed EX with `/CN=…` only (no `/O=`) still uses email identity, not display.
    #[test]
    fn tier2_5_typed_ex_cn_only_uses_email_identity() {
        let (sha, len) = hash_full_body("body");
        let r = CanonicalRecipient {
            recipient_type: crate::grouping::CanonicalRecipientType::To,
            display_name: Some("Alice Example (noisy)".into()),
            address_type: Some("EX".into()),
            email_address: Some("/CN=Recipients/CN=alice".into()),
            smtp_address: None,
        };
        assert!(r.identity_is_x500());
        assert_eq!(r.identity_key().as_deref(), Some("/CN=RECIPIENTS/CN=ALICE"));
        let recips = [r];
        let s = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("Alice Example (noisy)"),
            recipients: Some(&recips),
            ..Default::default()
        };
        let keys = compute_dedup_keys_ex(
            None,
            Some("S"),
            Some(1),
            Some("s@x.com"),
            Some("body"),
            &[],
            &s,
        );
        assert!(keys.strong_content_hash.is_some());
    }

    /// DoD-5: EX-only recipients (no SmtpAddress) with different display formatting merge.
    #[test]
    fn tier2_5_ex_only_recipients_merge_despite_display_noise() {
        let (sha, len) = hash_full_body("same body");
        let ex_dn = "/o=First Organization/ou=Exchange Administrative Group/cn=Recipients/cn=alice";
        let r1 = CanonicalRecipient {
            recipient_type: crate::grouping::CanonicalRecipientType::To,
            display_name: Some("Alice Example (noisy A)".into()),
            address_type: Some("EX".into()),
            email_address: Some(ex_dn.into()),
            smtp_address: None,
        };
        let r2 = CanonicalRecipient {
            recipient_type: crate::grouping::CanonicalRecipientType::To,
            display_name: Some("ALICE / Different Format".into()),
            address_type: Some("EX".into()),
            email_address: Some(ex_dn.to_ascii_uppercase()),
            smtp_address: None,
        };
        // Same DN identity keys.
        assert_eq!(r1.identity_key(), r2.identity_key());
        assert!(r1.identity_is_x500());
        assert!(r2.identity_is_x500());
        let recips_a = [r1];
        let recips_b = [r2];
        let s_a = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("Alice Example (noisy A)"),
            display_cc: None,
            display_bcc: None,
            recipients: Some(&recips_a),
            ignore_inline_attachments: false,
        };
        let s_b = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("ALICE / Different Format"),
            display_cc: None,
            display_bcc: None,
            recipients: Some(&recips_b),
            ignore_inline_attachments: false,
        };
        let k_a = compute_dedup_keys_ex(
            None,
            Some("S"),
            Some(1),
            Some("s@x.com"),
            Some("same body"),
            &[],
            &s_a,
        );
        let k_b = compute_dedup_keys_ex(
            None,
            Some("S"),
            Some(1),
            Some("s@x.com"),
            Some("same body"),
            &[],
            &s_b,
        );
        assert_eq!(
            k_a.strong_content_hash, k_b.strong_content_hash,
            "EX DN keys must merge despite display noise"
        );
        // Pure display path would split (different display strings).
        let s_disp_a = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("Alice Example (noisy A)"),
            ..Default::default()
        };
        let s_disp_b = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("ALICE / Different Format"),
            ..Default::default()
        };
        let k_da = compute_dedup_keys_ex(
            None,
            Some("S"),
            Some(1),
            Some("s@x.com"),
            Some("same body"),
            &[],
            &s_disp_a,
        );
        let k_db = compute_dedup_keys_ex(
            None,
            Some("S"),
            Some(1),
            Some("s@x.com"),
            Some("same body"),
            &[],
            &s_disp_b,
        );
        assert_ne!(
            k_da.strong_content_hash, k_db.strong_content_hash,
            "display-only path must still split on display noise"
        );
    }

    /// SMTP case-fold still merges under structured table path.
    #[test]
    fn tier2_5_smtp_structured_case_fold_merges() {
        let (sha, len) = hash_full_body("body");
        let a = [CanonicalRecipient {
            recipient_type: crate::grouping::CanonicalRecipientType::To,
            display_name: Some("Bob".into()),
            address_type: Some("SMTP".into()),
            email_address: Some("Bob@Example.COM".into()),
            smtp_address: Some("Bob@Example.COM".into()),
        }];
        let b = [CanonicalRecipient {
            recipient_type: crate::grouping::CanonicalRecipientType::To,
            display_name: Some("bob".into()),
            address_type: Some("SMTP".into()),
            email_address: Some("bob@example.com".into()),
            smtp_address: None,
        }];
        let s1 = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            recipients: Some(&a),
            ..Default::default()
        };
        let s2 = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            recipients: Some(&b),
            ..Default::default()
        };
        let k1 = compute_dedup_keys_ex(None, Some("S"), None, None, Some("body"), &[], &s1);
        let k2 = compute_dedup_keys_ex(None, Some("S"), None, None, Some("body"), &[], &s2);
        assert_eq!(k1.strong_content_hash, k2.strong_content_hash);
    }

    /// Empty structured table falls back to display-string path (no behavior change).
    #[test]
    fn tier2_5_empty_table_uses_display_path() {
        let (sha, len) = hash_full_body("body");
        let empty: [CanonicalRecipient; 0] = [];
        let s_table = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("a@x.com; b@y.com"),
            recipients: Some(&empty),
            ..Default::default()
        };
        let s_disp = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("b@y.com;a@x.com"),
            recipients: None,
            ..Default::default()
        };
        let k1 = compute_dedup_keys_ex(None, Some("S"), None, None, Some("body"), &[], &s_table);
        let k2 = compute_dedup_keys_ex(None, Some("S"), None, None, Some("body"), &[], &s_disp);
        assert_eq!(k1.strong_content_hash, k2.strong_content_hash);
    }

    /// Non-empty table with no usable identity keys falls back to display path.
    #[test]
    fn tier2_5_empty_identity_keys_fall_back_to_display() {
        let (sha, len) = hash_full_body("body");
        // Rows with no smtp/email/display → identity_key() is None.
        let blank = [CanonicalRecipient {
            recipient_type: crate::grouping::CanonicalRecipientType::To,
            display_name: None,
            address_type: None,
            email_address: None,
            smtp_address: None,
        }];
        let s_blank_table = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("alice@example.com"),
            display_cc: None,
            display_bcc: None,
            recipients: Some(&blank),
            ignore_inline_attachments: false,
        };
        let s_display = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("alice@example.com"),
            recipients: None,
            ..Default::default()
        };
        let k_blank = compute_dedup_keys_ex(
            None,
            Some("S"),
            None,
            None,
            Some("body"),
            &[],
            &s_blank_table,
        );
        let k_disp =
            compute_dedup_keys_ex(None, Some("S"), None, None, Some("body"), &[], &s_display);
        assert_eq!(
            k_blank.strong_content_hash, k_disp.strong_content_hash,
            "empty identity keys must not produce empty structured fingerprint"
        );
        // Fingerprint helper agrees.
        let fp_blank =
            recipients_fingerprint_ex(Some("alice@example.com"), None, None, Some(&blank));
        let fp_disp = recipients_fingerprint_ex(Some("alice@example.com"), None, None, None);
        assert_eq!(fp_blank, fp_disp);
        // Distinct display still splits under the fallback path.
        let s_other = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("bob@example.com"),
            recipients: Some(&blank),
            ..Default::default()
        };
        let k_other =
            compute_dedup_keys_ex(None, Some("S"), None, None, Some("body"), &[], &s_other);
        assert_ne!(k_blank.strong_content_hash, k_other.strong_content_hash);
    }

    /// Dual BCC policy: identical messages except structured Bcc → different Tier-2.5.
    #[test]
    fn tier2_5_structured_bcc_participates_in_identity() {
        let (sha, len) = hash_full_body("same body");
        let to_only = [CanonicalRecipient {
            recipient_type: crate::grouping::CanonicalRecipientType::To,
            display_name: Some("Alice".into()),
            address_type: Some("SMTP".into()),
            email_address: Some("alice@example.com".into()),
            smtp_address: Some("alice@example.com".into()),
        }];
        let to_and_bcc = [
            CanonicalRecipient {
                recipient_type: crate::grouping::CanonicalRecipientType::To,
                display_name: Some("Alice".into()),
                address_type: Some("SMTP".into()),
                email_address: Some("alice@example.com".into()),
                smtp_address: Some("alice@example.com".into()),
            },
            CanonicalRecipient {
                recipient_type: crate::grouping::CanonicalRecipientType::Bcc,
                display_name: Some("Secret".into()),
                address_type: Some("SMTP".into()),
                email_address: Some("secret@example.com".into()),
                smtp_address: Some("secret@example.com".into()),
            },
        ];
        let s_to = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("Alice <alice@example.com>"),
            display_cc: None,
            display_bcc: None,
            recipients: Some(&to_only),
            ignore_inline_attachments: false,
        };
        let s_bcc = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("Alice <alice@example.com>"),
            display_cc: None,
            // Display BCC absent — identity difference comes from structured table.
            display_bcc: None,
            recipients: Some(&to_and_bcc),
            ignore_inline_attachments: false,
        };
        let k_to = compute_dedup_keys_ex(
            None,
            Some("S"),
            Some(1),
            Some("s@x.com"),
            Some("same body"),
            &[],
            &s_to,
        );
        let k_bcc = compute_dedup_keys_ex(
            None,
            Some("S"),
            Some(1),
            Some("s@x.com"),
            Some("same body"),
            &[],
            &s_bcc,
        );
        assert_ne!(
            k_to.strong_content_hash, k_bcc.strong_content_hash,
            "structured Bcc must change Tier-2.5 hash when table path is used"
        );
        assert_ne!(
            k_to.fp_recipients, k_bcc.fp_recipients,
            "recipient fingerprint must differ when Bcc row present"
        );
    }

    // ── 0086 attach-content identity ─────────────────────────────────────────

    fn keys_attach(body: &str, atts: &[AttachmentInfo], ignore_inline: bool) -> DedupKeys {
        let (sha, len) = hash_full_body(body);
        let strong = StrongHashInput {
            identity: IdentityLevel::BodyRecipAttach,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("a@x.com"),
            ignore_inline_attachments: ignore_inline,
            ..Default::default()
        };
        compute_dedup_keys_ex(
            None,
            Some("Subject"),
            Some(1),
            Some("s@x.com"),
            Some(body),
            atts,
            &strong,
        )
    }

    fn keys_body_recip(body: &str, atts: &[AttachmentInfo]) -> DedupKeys {
        let (sha, len) = hash_full_body(body);
        let strong = StrongHashInput {
            identity: IdentityLevel::BodyRecip,
            body_sha256: Some(&sha),
            body_char_len: Some(len),
            display_to: Some("a@x.com"),
            ..Default::default()
        };
        compute_dedup_keys_ex(
            None,
            Some("Subject"),
            Some(1),
            Some("s@x.com"),
            Some(body),
            atts,
            &strong,
        )
    }

    fn digest_of(bytes: &[u8]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize().into()
    }

    #[test]
    fn attach_content_different_real_digests_split() {
        let mut a = AttachmentInfo::new("file.bin", 4);
        a.content_sha256 = Some(digest_of(b"AAAA"));
        let mut b = AttachmentInfo::new("file.bin", 4);
        b.content_sha256 = Some(digest_of(b"BBBB"));
        let k_a = keys_attach("same body", &[a], false);
        let k_b = keys_attach("same body", &[b], false);
        assert_ne!(
            k_a.strong_content_hash, k_b.strong_content_hash,
            "different attach bytes must split at BodyRecipAttach"
        );
        // Equal digests merge.
        let mut c = AttachmentInfo::new("file.bin", 4);
        c.content_sha256 = Some(digest_of(b"AAAA"));
        let mut d = AttachmentInfo::new("file.bin", 4);
        d.content_sha256 = Some(digest_of(b"AAAA"));
        let k_c = keys_attach("same body", &[c], false);
        let k_d = keys_attach("same body", &[d], false);
        assert_eq!(k_c.strong_content_hash, k_d.strong_content_hash);
    }

    #[test]
    fn attach_content_order_independent() {
        let mut a = AttachmentInfo::new("a.txt", 1);
        a.content_sha256 = Some(digest_of(b"1"));
        let mut b = AttachmentInfo::new("b.txt", 1);
        b.content_sha256 = Some(digest_of(b"2"));
        let k1 = keys_attach("body", &[a.clone(), b.clone()], false);
        let k2 = keys_attach("body", &[b, a], false);
        assert_eq!(
            k1.strong_content_hash, k2.strong_content_hash,
            "attach digest order must not affect strong hash"
        );
    }

    #[test]
    fn choice_b_unread_sentinel_formula_and_distinctness() {
        // Exact formula freeze.
        let mut expected = Sha256::new();
        expected.update(ATTACH_UNREAD_DOMAIN);
        expected.update(b"contract.pdf");
        expected.update(b"\0");
        expected.update(100u32.to_le_bytes());
        let expected: [u8; 32] = expected.finalize().into();
        assert_eq!(
            attach_unread_sentinel("Contract.pdf", 100),
            expected,
            "sentinel must use lowercase name + LE size + domain tag"
        );
        assert_eq!(
            attach_unread_sentinel("Contract.pdf", 100),
            attach_unread_sentinel("contract.PDF", 100),
            "name case must fold"
        );

        let unread_pdf = attach_unread_sentinel("Contract.pdf", 100);
        let unread_xlsx = attach_unread_sentinel("Financials.xlsx", 100);
        let empty = EMPTY_CONTENT_SHA256;
        let real = digest_of(b"real-bytes");
        assert_ne!(unread_pdf, unread_xlsx);
        assert_ne!(unread_pdf, empty);
        assert_ne!(unread_xlsx, empty);
        assert_ne!(unread_pdf, real);
        assert_ne!(unread_xlsx, real);
        assert_ne!(empty, real);
        // Same name+size unreads match.
        assert_eq!(
            attach_unread_sentinel("Contract.pdf", 100),
            attach_unread_sentinel("contract.pdf", 100)
        );
        // Slot helper: None → sentinel; is_unread forces sentinel even if Some.
        assert_eq!(
            attach_content_slot("Contract.pdf", 100, None, false),
            unread_pdf
        );
        assert_eq!(
            attach_content_slot("Contract.pdf", 100, Some(real), true),
            unread_pdf
        );
        assert_eq!(
            attach_content_slot("Contract.pdf", 100, Some(real), false),
            real
        );
    }

    #[test]
    fn choice_b_no_tier_hijack_unread_vs_no_attach() {
        // Message with unread attach must not share strong hash with no-attach peer
        // that matches body+recip (Choice A regression guard).
        let mut with_attach = AttachmentInfo::new("Contract.pdf", 100);
        // content_sha256 None → sentinel at BodyRecipAttach.
        with_attach.content_sha256 = None;
        let k_unread = keys_attach("same body", &[with_attach], false);
        let k_none = keys_attach("same body", &[], false);
        assert_ne!(
            k_unread.strong_content_hash, k_none.strong_content_hash,
            "unread attach must not merge with no-attach body-recip peer"
        );
        // At body-recip alone they would still share the same strong hash
        // (name:size is in v1 preimage so actually they differ at body-recip too
        // because name:size is in v1). Use equal name:size path via empty digests
        // omit-would-have-merged: two different unread names would collapse if
        // omit-None were used with empty attach lists — already covered.
        // Stronger check: body-recip without attach-content can share when meta
        // matches; inject same name:size on both and ensure attach level still
        // distinguishes unread sentinel from real empty only when policy says so.
        let mut a = AttachmentInfo::new("f.bin", 0);
        a.content_sha256 = None; // unread
        let mut b = AttachmentInfo::new("f.bin", 0);
        b.content_sha256 = Some(EMPTY_CONTENT_SHA256); // legitimate empty
        let k_a = keys_attach("body", &[a], false);
        let k_b = keys_attach("body", &[b], false);
        assert_ne!(
            k_a.strong_content_hash, k_b.strong_content_hash,
            "unread size-0 must not equal legitimate empty digest"
        );
        // body-recip (no attach content) merges them (same name:size).
        let mut a2 = AttachmentInfo::new("f.bin", 0);
        a2.content_sha256 = None;
        let mut b2 = AttachmentInfo::new("f.bin", 0);
        b2.content_sha256 = Some(EMPTY_CONTENT_SHA256);
        let br_a = keys_body_recip("body", &[a2]);
        let br_b = keys_body_recip("body", &[b2]);
        assert_eq!(
            br_a.strong_content_hash, br_b.strong_content_hash,
            "body-recip ignores content digests"
        );
    }

    #[test]
    fn empty_vs_length_mismatch_slot_policy() {
        // Legitimate empty: size 0 + empty digest.
        assert_eq!(digest_of(b""), EMPTY_CONTENT_SHA256);
        let mut empty_ok = AttachmentInfo::new("empty.bin", 0);
        empty_ok.content_sha256 = Some(EMPTY_CONTENT_SHA256);
        // Length mismatch: size > 0, treat as unread (not empty digest).
        let mut mismatch = AttachmentInfo::new("empty.bin", 10);
        mismatch.content_sha256 = Some(attach_unread_sentinel("empty.bin", 10));
        let k_empty = keys_attach("body", &[empty_ok], false);
        let k_mm = keys_attach("body", &[mismatch], false);
        assert_ne!(k_empty.strong_content_hash, k_mm.strong_content_hash);
        // Direct sentinel ≠ empty digest.
        assert_ne!(
            attach_unread_sentinel("empty.bin", 10),
            EMPTY_CONTENT_SHA256
        );
    }

    #[test]
    fn nist_sha256_multi_block_kat() {
        // NIST/FIPS known-answer vectors on the same sha2::Sha256 path used for
        // attach streaming (guards RUSTSEC-2021-0100-class multi-block miscompute).
        // "abc"
        let mut h = Sha256::new();
        h.update(b"abc");
        let abc: [u8; 32] = h.finalize().into();
        assert_eq!(
            hex(&abc),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 1_000_000 × 'a' (multi-block)
        let mut h = Sha256::new();
        let chunk = vec![b'a'; 8192];
        let mut remaining = 1_000_000usize;
        while remaining > 0 {
            let n = remaining.min(chunk.len());
            h.update(&chunk[..n]);
            remaining -= n;
        }
        let million_a: [u8; 32] = h.finalize().into();
        assert_eq!(
            hex(&million_a),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        // Stream-style chunked hash matches one-shot for arbitrary payload.
        let payload: Vec<u8> = (0u8..200).cycle().take(200_000).collect();
        let one_shot = digest_of(&payload);
        let mut h = Sha256::new();
        for part in payload.chunks(64 * 1024) {
            h.update(part);
        }
        let chunked: [u8; 32] = h.finalize().into();
        assert_eq!(one_shot, chunked);
    }

    #[test]
    fn attach_content_inline_ignored_when_flag_set() {
        let mut inline = AttachmentInfo::new("logo.png", 50);
        inline.is_inline = true;
        inline.content_sha256 = Some(digest_of(b"logo-a"));
        let mut inline_b = AttachmentInfo::new("logo.png", 50);
        inline_b.is_inline = true;
        inline_b.content_sha256 = Some(digest_of(b"logo-b"));
        // With ignore: different inline digests must not split.
        let k1 = keys_attach("body", &[inline.clone()], true);
        let k2 = keys_attach("body", &[inline_b], true);
        assert_eq!(
            k1.strong_content_hash, k2.strong_content_hash,
            "ignored inline must not participate in attach-content slots"
        );
        // Without ignore: different digests split.
        let mut i1 = AttachmentInfo::new("logo.png", 50);
        i1.is_inline = true;
        i1.content_sha256 = Some(digest_of(b"logo-a"));
        let mut i2 = AttachmentInfo::new("logo.png", 50);
        i2.is_inline = true;
        i2.content_sha256 = Some(digest_of(b"logo-b"));
        let k3 = keys_attach("body", &[i1], false);
        let k4 = keys_attach("body", &[i2], false);
        assert_ne!(k3.strong_content_hash, k4.strong_content_hash);
    }

    #[test]
    fn body_recip_attach_refines_body_recip() {
        // Equal body-recip with different real digests → attach level subdivides.
        let mut a = AttachmentInfo::new("doc.pdf", 4);
        a.content_sha256 = Some(digest_of(b"AAAA"));
        let mut b = AttachmentInfo::new("doc.pdf", 4);
        b.content_sha256 = Some(digest_of(b"BBBB"));
        let br_a = keys_body_recip("body", &[a.clone()]);
        let br_b = keys_body_recip("body", &[b.clone()]);
        assert_eq!(
            br_a.strong_content_hash, br_b.strong_content_hash,
            "body-recip ignores digests (same name:size)"
        );
        assert_eq!(br_a.content_hash, br_b.content_hash);
        let att_a = keys_attach("body", &[a], false);
        let att_b = keys_attach("body", &[b], false);
        assert_ne!(att_a.strong_content_hash, att_b.strong_content_hash);
        // equal-v2 ⇒ equal-v1 still holds for each side.
        assert_eq!(att_a.content_hash, br_a.content_hash);
        assert_eq!(att_b.content_hash, br_b.content_hash);
    }

    #[test]
    fn none_content_sha256_fills_sentinel_not_omit() {
        // Two messages with different unread attach names must not collapse
        // via empty attach-content tails (omit-None regression).
        let a = AttachmentInfo::new("Contract.pdf", 100);
        let b = AttachmentInfo::new("Financials.xlsx", 100);
        let k_a = keys_attach("body", &[a], false);
        let k_b = keys_attach("body", &[b], false);
        assert_ne!(
            k_a.strong_content_hash, k_b.strong_content_hash,
            "None digests must synthesize distinct name+size sentinels"
        );
        // Identical unread slots still match.
        let c = AttachmentInfo::new("Contract.pdf", 100);
        let d = AttachmentInfo::new("contract.PDF", 100);
        let k_c = keys_attach("body", &[c], false);
        let k_d = keys_attach("body", &[d], false);
        assert_eq!(k_c.strong_content_hash, k_d.strong_content_hash);
    }
}
