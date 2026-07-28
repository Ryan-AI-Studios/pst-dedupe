//! Tiered hashing strategy for email deduplication (0076 hardened).
//!
//! Tier 1: normalized Message-ID.
//! Tier 2 (v1): subject | submit | sender | ≤4096 **chars** body preview | attach name:size.
//! Tier 2.5 (v2): v1 preimage bytes unchanged, then optional full-body / recipients / attach content.

use sha2::{Digest, Sha256};

use crate::grouping::{normalize_recipients, IdentityLevel};

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
    /// Optional content SHA-256 when `--strong-content-hash body-recip-attach` and probe succeeded.
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

    let fp_recipients =
        recipients_fingerprint(strong.display_to, strong.display_cc, strong.display_bcc);

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

/// Compute recipient-component fingerprint (attribution).
pub fn recipients_fingerprint(
    display_to: Option<&str>,
    display_cc: Option<&str>,
    display_bcc: Option<&str>,
) -> u64 {
    let mut h = Sha256::new();
    h.update(normalize_recipients(display_to.unwrap_or("")).as_bytes());
    h.update(b"|");
    h.update(normalize_recipients(display_cc.unwrap_or("")).as_bytes());
    h.update(b"|");
    h.update(normalize_recipients(display_bcc.unwrap_or("")).as_bytes());
    fingerprint64(&h.finalize())
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
        hasher.update(normalize_recipients(strong.display_to.unwrap_or("")).as_bytes());
        hasher.update(b"|");
        hasher.update(normalize_recipients(strong.display_cc.unwrap_or("")).as_bytes());
        hasher.update(b"|");
        hasher.update(normalize_recipients(strong.display_bcc.unwrap_or("")).as_bytes());
        hasher.update(b"|");
    }
    if strong.identity.includes_attach_content() {
        let mut digests: Vec<[u8; 32]> = attachments
            .iter()
            .filter(|a| !(strong.ignore_inline_attachments && a.is_inline))
            .filter_map(|a| a.content_sha256)
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
}
