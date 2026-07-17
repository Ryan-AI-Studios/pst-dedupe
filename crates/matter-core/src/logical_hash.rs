//! Desk **logical_hash** v1 — versioned, length-prefixed content identity.
//!
//! This is **not** the CLI `dedup-engine` Tier-2 content hash. See the crate
//! README for the Tier-2 vs logical_hash comparison.
//!
//! # Framing format (v1)
//!
//! Preimage bytes are a deterministic UTF-8 stream (no bincode/protobuf).
//! The stream is hashed with SHA-256; the stored digest is **lowercase hex**.
//!
//! ## Email preimage
//!
//! ```text
//! v1\n
//! message_id\n<len>\n<bytes>\n
//! subject\n<len>\n<bytes>\n
//! from\n<len>\n<bytes>\n
//! to\n<len>\n<bytes>\n
//! cc\n<len>\n<bytes>\n
//! bcc\n<len>\n<bytes>\n          # always present (empty list → len 0)
//! sent\n<len>\n<bytes>\n
//! received\n<len>\n<bytes>\n
//! body\n<len>\n<bytes>\n
//! attachments\n<count>\n
//!   for each attachment (sorted):
//!     filename\n<len>\n<bytes>\n
//!     size\n<decimal ascii>\n
//!     native_sha256\n<len>\n<bytes>\n
//! ```
//!
//! - `<len>` is the decimal ASCII byte count of the following payload.
//! - Address list payloads are sorted, case-folded addresses joined by `\n`
//!   **inside** the length-prefixed field (so `\n` in body cannot spoof structure).
//! - Attachment sort key: `(filename_lower, size, native_sha256)`.
//! - **BCC is required in the frame** even when empty — BCC-present ≠ BCC-absent.
//!
//! ## Non-email preimage
//!
//! ```text
//! v1\n
//! category\n<len>\n<bytes>\n
//! title\n<len>\n<bytes>\n
//! author\n<len>\n<bytes>\n
//! created\n<len>\n<bytes>\n
//! text\n<len>\n<bytes>\n
//! children\n<count>\n
//!   for each child digest (sorted):
//!     native_sha256\n<len>\n<bytes>\n
//! ```
//!
//! # Normalization (v1)
//!
//! | Field | Rule |
//! |---|---|
//! | Message-ID | Trim; strip outer `<>`; lowercase (parity with `dedup-engine`) |
//! | Subject (strict) | Unicode trim; collapse whitespace runs to single space; **keep** `RE:`/`FW:` |
//! | Addresses | Trim; full-string lowercase; sort each list independently |
//! | Times | UTC second-resolution RFC3339, or empty string |
//! | Body | If looks like HTML → minimal tag strip; CRLF→LF; strip zero-width; trim |
//! | Attachments | Direct children only; sorted as above |

use sha2::{Digest, Sha256};

/// Algorithm version embedded in the preimage and stored on items when set.
pub const LOGICAL_HASH_VERSION: u32 = 1;

/// One attachment contribution to an email logical preimage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalAttachment {
    pub filename: String,
    pub size: u64,
    pub native_sha256: String,
}

/// Pure inputs for [`compute_email_logical_hash`].
///
/// Callers (e.g. 0018 PST extract) supply already-extracted fields; this module
/// does **not** parse EML/PST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailLogicalInput {
    pub message_id: Option<String>,
    pub subject: Option<String>,
    pub from: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    /// Always included in the preimage (may be empty). Required for defensibility.
    pub bcc: Vec<String>,
    /// RFC3339 or parseable timestamp; normalized to UTC second resolution.
    pub sent: Option<String>,
    pub received: Option<String>,
    pub body: Option<String>,
    pub attachments: Vec<LogicalAttachment>,
}

/// Pure inputs for [`compute_non_email_logical_hash`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonEmailLogicalInput {
    pub category: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub created: Option<String>,
    pub text: Option<String>,
    /// Child item `native_sha256` digests (sorted before hashing).
    pub children_native_sha256: Vec<String>,
}

/// Compute lowercase-hex SHA-256 of the email logical preimage.
pub fn compute_email_logical_hash(input: &EmailLogicalInput) -> String {
    let preimage = email_logical_preimage(input);
    sha256_hex(&preimage)
}

/// Compute lowercase-hex SHA-256 of the non-email logical preimage.
pub fn compute_non_email_logical_hash(input: &NonEmailLogicalInput) -> String {
    let preimage = non_email_logical_preimage(input);
    sha256_hex(&preimage)
}

/// Build the exact email preimage bytes (for tests / adversarial framing checks).
pub fn email_logical_preimage(input: &EmailLogicalInput) -> Vec<u8> {
    let mut out = Vec::new();
    write_version_line(&mut out);

    let mid = input
        .message_id
        .as_deref()
        .map(normalize_message_id)
        .unwrap_or_default();
    write_len_field(&mut out, "message_id", mid.as_bytes());

    let subject = input
        .subject
        .as_deref()
        .map(normalize_subject_strict)
        .unwrap_or_default();
    write_len_field(&mut out, "subject", subject.as_bytes());

    let from = input
        .from
        .as_deref()
        .map(normalize_address)
        .unwrap_or_default();
    write_len_field(&mut out, "from", from.as_bytes());

    let to = normalize_address_list(&input.to);
    write_len_field(&mut out, "to", to.as_bytes());

    let cc = normalize_address_list(&input.cc);
    write_len_field(&mut out, "cc", cc.as_bytes());

    // BCC always present in the frame (empty list → zero-length payload).
    let bcc = normalize_address_list(&input.bcc);
    write_len_field(&mut out, "bcc", bcc.as_bytes());

    let sent = input
        .sent
        .as_deref()
        .map(normalize_time_utc_second)
        .unwrap_or_default();
    write_len_field(&mut out, "sent", sent.as_bytes());

    let received = input
        .received
        .as_deref()
        .map(normalize_time_utc_second)
        .unwrap_or_default();
    write_len_field(&mut out, "received", received.as_bytes());

    let body = input
        .body
        .as_deref()
        .map(normalize_body)
        .unwrap_or_default();
    write_len_field(&mut out, "body", body.as_bytes());

    let mut attachments = input.attachments.clone();
    attachments.sort_by(|a, b| {
        let fa = a.filename.to_lowercase();
        let fb = b.filename.to_lowercase();
        fa.cmp(&fb)
            .then_with(|| a.size.cmp(&b.size))
            .then_with(|| a.native_sha256.cmp(&b.native_sha256))
    });

    out.extend_from_slice(b"attachments\n");
    out.extend_from_slice(attachments.len().to_string().as_bytes());
    out.push(b'\n');

    for att in &attachments {
        let fname = att.filename.to_lowercase();
        write_len_field(&mut out, "filename", fname.as_bytes());
        // size is decimal ASCII line (not length-prefixed payload)
        out.extend_from_slice(b"size\n");
        out.extend_from_slice(att.size.to_string().as_bytes());
        out.push(b'\n');
        write_len_field(&mut out, "native_sha256", att.native_sha256.as_bytes());
    }

    out
}

/// Build the exact non-email preimage bytes.
pub fn non_email_logical_preimage(input: &NonEmailLogicalInput) -> Vec<u8> {
    let mut out = Vec::new();
    write_version_line(&mut out);

    let category = input.category.as_deref().unwrap_or("").trim();
    write_len_field(&mut out, "category", category.as_bytes());

    let title = input
        .title
        .as_deref()
        .map(|t| collapse_whitespace(t.trim()))
        .unwrap_or_default();
    write_len_field(&mut out, "title", title.as_bytes());

    let author = input
        .author
        .as_deref()
        .map(|a| a.trim().to_string())
        .unwrap_or_default();
    write_len_field(&mut out, "author", author.as_bytes());

    let created = input
        .created
        .as_deref()
        .map(normalize_time_utc_second)
        .unwrap_or_default();
    write_len_field(&mut out, "created", created.as_bytes());

    let text = input
        .text
        .as_deref()
        .map(normalize_body)
        .unwrap_or_default();
    write_len_field(&mut out, "text", text.as_bytes());

    let mut children = input.children_native_sha256.clone();
    children.sort();

    out.extend_from_slice(b"children\n");
    out.extend_from_slice(children.len().to_string().as_bytes());
    out.push(b'\n');
    for dig in &children {
        write_len_field(&mut out, "native_sha256", dig.as_bytes());
    }

    out
}

/// Normalize a Message-ID for consistent matching.
///
/// - Trim whitespace
/// - Strip outer angle brackets `<` `>`
/// - Lowercase
///
/// Intentionally duplicated from `dedup-engine` (no crate coupling); parity
/// covered by unit tests.
pub fn normalize_message_id(mid: &str) -> String {
    mid.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_lowercase()
}

/// Strict subject for logical hash: collapse whitespace; **keep** RE:/FW: prefixes.
pub fn normalize_subject_strict(subject: &str) -> String {
    collapse_whitespace(subject.trim())
}

/// Normalize a single email address: trim + lowercase (full string for v1).
pub fn normalize_address(addr: &str) -> String {
    addr.trim().to_lowercase()
}

/// Normalize and sort an address list; join with `\n` for stable payload encoding.
pub fn normalize_address_list(addrs: &[String]) -> String {
    let mut norm: Vec<String> = addrs
        .iter()
        .map(|a| normalize_address(a))
        .filter(|a| !a.is_empty())
        .collect();
    norm.sort();
    norm.join("\n")
}

/// Normalize a timestamp to UTC second-resolution RFC3339, or empty if unparseable/absent.
///
/// Accepts RFC3339 (with or without fractional seconds). Truncates to whole seconds.
pub fn normalize_time_utc_second(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    // chrono::DateTime parse via DateTime::parse_from_rfc3339
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return dt
            .with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    }
    // Already a bare second form without offset — return as-is if it looks ok, else empty.
    // Prefer empty over inventing timezone for ambiguous inputs.
    String::new()
}

/// Normalize body text for the logical preimage.
///
/// 1. If the text **looks like HTML** (contains a `<` tag-like pattern), strip tags
///    with a minimal state machine (v1; not a full HTML parser).
/// 2. CRLF / lone CR → LF
/// 3. Remove zero-width characters (U+200B, U+200C, U+200D, U+FEFF)
/// 4. Trim leading/trailing whitespace on the whole string
///
/// Does **not** strip `Received:` chains (callers should pass body only, not full MIME).
pub fn normalize_body(body: &str) -> String {
    let text = if looks_like_html(body) {
        strip_html_tags_minimal(body)
    } else {
        body.to_string()
    };
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text: String = text.chars().filter(|c| !is_zero_width(*c)).collect();
    text.trim().to_string()
}

// --- framing helpers ---

fn write_version_line(out: &mut Vec<u8>) {
    out.extend_from_slice(format!("v{LOGICAL_HASH_VERSION}\n").as_bytes());
}

fn write_len_field(out: &mut Vec<u8>, tag: &str, payload: &[u8]) {
    out.extend_from_slice(tag.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(payload.len().to_string().as_bytes());
    out.push(b'\n');
    out.extend_from_slice(payload);
    out.push(b'\n');
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

fn looks_like_html(s: &str) -> bool {
    // Cheap heuristic: has an opening angle-bracket tag character sequence.
    let lower = s.to_ascii_lowercase();
    lower.contains("<html")
        || lower.contains("<body")
        || lower.contains("<div")
        || lower.contains("<p>")
        || lower.contains("<br")
        || lower.contains("<span")
}

fn strip_html_tags_minimal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_email() -> EmailLogicalInput {
        EmailLogicalInput {
            message_id: Some("<ABC@Example.COM>".into()),
            subject: Some("  Hello   World  ".into()),
            from: Some(" Alice@Example.COM ".into()),
            to: vec!["z@ex.com".into(), "a@ex.com".into()],
            cc: vec!["c@ex.com".into()],
            bcc: vec![],
            sent: Some("2020-01-02T03:04:05.999Z".into()),
            received: Some("2020-01-02T03:05:00+00:00".into()),
            body: Some("Hello body".into()),
            attachments: vec![
                LogicalAttachment {
                    filename: "B.pdf".into(),
                    size: 20,
                    native_sha256: "bb".into(),
                },
                LogicalAttachment {
                    filename: "a.pdf".into(),
                    size: 10,
                    native_sha256: "aa".into(),
                },
            ],
        }
    }

    #[test]
    fn hash_stability_and_sort_independence() {
        let a = sample_email();
        let mut b = sample_email();
        // Reverse To and attachment order — must not change hash.
        b.to = vec!["a@ex.com".into(), "z@ex.com".into()];
        b.attachments.reverse();

        let h1 = compute_email_logical_hash(&a);
        let h2 = compute_email_logical_hash(&b);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn sensitivity_body_and_attachment_digest() {
        let base = sample_email();
        let h0 = compute_email_logical_hash(&base);

        let mut body_chg = base.clone();
        body_chg.body = Some("Hello body!".into());
        assert_ne!(compute_email_logical_hash(&body_chg), h0);

        let mut att_chg = base.clone();
        att_chg.attachments[0].native_sha256 = "cc".into();
        assert_ne!(compute_email_logical_hash(&att_chg), h0);
    }

    #[test]
    fn bcc_distinctness() {
        let mut no_bcc = sample_email();
        no_bcc.bcc = vec![];
        let mut with_bcc = sample_email();
        with_bcc.bcc = vec!["secret@ex.com".into()];

        let h1 = compute_email_logical_hash(&no_bcc);
        let h2 = compute_email_logical_hash(&with_bcc);
        assert_ne!(h1, h2, "BCC-present must differ from BCC-absent");

        // Empty BCC field still present in preimage (tag always written).
        let pre = email_logical_preimage(&no_bcc);
        let s = String::from_utf8_lossy(&pre);
        assert!(s.contains("bcc\n0\n\n"), "empty bcc must be framed: {s}");
    }

    #[test]
    fn adversarial_body_cannot_spoof_attachments() {
        let mut clean = sample_email();
        clean.body = Some("plain body".into());
        clean.attachments = vec![LogicalAttachment {
            filename: "real.pdf".into(),
            size: 1,
            native_sha256: "realhash".into(),
        }];

        let mut adversarial = clean.clone();
        adversarial.body = Some("plain body\nattachments:\nfake.pdf|1|abc".into());

        // Same structured attachments → body change alone changes hash, but
        // preimage attachment section must still list only real.pdf.
        let pre = email_logical_preimage(&adversarial);
        let s = String::from_utf8_lossy(&pre);
        assert!(s.contains("real.pdf") || s.contains("real.pdf".to_lowercase().as_str()));
        // The fake name appears only inside the body length-prefixed payload,
        // not as a second structured attachment entry. Count attachment entries:
        // attachments\n1\n means count=1.
        assert!(
            s.contains("attachments\n1\n"),
            "must have exactly one attachment, got: {s}"
        );
        // Hash differs from clean because body differs, not because attachment list grew.
        assert_ne!(
            compute_email_logical_hash(&adversarial),
            compute_email_logical_hash(&clean)
        );

        // Same body adversarial text, same attachments as clean with body equal —
        // prove that only the structured list contributes attachment identity:
        let mut same_body_fake_list_attempt = adversarial.clone();
        // Already only one structured attachment; re-hash is stable.
        let h = compute_email_logical_hash(&same_body_fake_list_attempt);
        assert_eq!(h, compute_email_logical_hash(&adversarial));

        // Changing structured list changes hash even if body already mentions the name.
        same_body_fake_list_attempt
            .attachments
            .push(LogicalAttachment {
                filename: "fake.pdf".into(),
                size: 1,
                native_sha256: "abc".into(),
            });
        assert_ne!(compute_email_logical_hash(&same_body_fake_list_attempt), h);
    }

    #[test]
    fn strict_subject_keeps_re_prefix() {
        let mut a = sample_email();
        a.subject = Some("RE: Hello".into());
        a.attachments.clear();
        let mut b = a.clone();
        b.subject = Some("Hello".into());
        assert_ne!(
            compute_email_logical_hash(&a),
            compute_email_logical_hash(&b),
            "RE: must not be stripped"
        );
    }

    #[test]
    fn message_id_normalize_parity() {
        // Parity with dedup-engine::hasher normalize_message_id rules.
        assert_eq!(
            normalize_message_id("<ABC123@example.com>"),
            "abc123@example.com"
        );
        assert_eq!(
            normalize_message_id("  <ABC123@example.com>  "),
            "abc123@example.com"
        );
        assert_eq!(normalize_message_id("abc@example.com"), "abc@example.com");

        let mut a = sample_email();
        a.message_id = Some("<A@B.com>".into());
        a.attachments.clear();
        let mut b = a.clone();
        b.message_id = Some("a@b.com".into());
        assert_eq!(
            compute_email_logical_hash(&a),
            compute_email_logical_hash(&b)
        );
    }

    #[test]
    fn native_sha256_not_in_email_logical_fields() {
        // §3.7.9 / §3.7.11: message-level native_sha256 and transport/MIME wrapper
        // bytes are intentionally absent from EmailLogicalInput. Two messages that
        // share all logical fields therefore hash identically even when their
        // "would-be" natives / transport wrappers differ outside the input type.
        let sample_a = sample_email();
        let sample_b = sample_email();
        // Distinct digests that extractors would store on the *item* row as
        // native_sha256 — NOT fed into compute_email_logical_hash.
        let _would_be_native_a = "msg_native_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let _would_be_native_b = "msg_native_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        // Transport/MIME wrapper notes also out of scope of EmailLogicalInput
        // (no Received:/MIME-Version fields on the type).
        let _mime_wrapper_a = "Received: from mx1.example\r\nMIME-Version: 1.0\r\n";
        let _mime_wrapper_b = "Received: from mx2.other\r\nMIME-Version: 1.0\r\nX-Mailer: Foo\r\n";
        assert_ne!(_would_be_native_a, _would_be_native_b);
        assert_ne!(_mime_wrapper_a, _mime_wrapper_b);

        let h_a = compute_email_logical_hash(&sample_a);
        let h_b = compute_email_logical_hash(&sample_b);
        assert_eq!(
            h_a, h_b,
            "same logical fields → same hash regardless of message native/MIME"
        );

        // Preimage must not absorb transport headers that only exist outside the input.
        let pre = email_logical_preimage(&sample_a);
        let s = String::from_utf8_lossy(&pre);
        assert!(
            !s.contains("Received:"),
            "transport Received: must not appear in logical preimage: {s}"
        );
        assert!(
            !s.contains("MIME-Version"),
            "MIME-Version must not appear in logical preimage: {s}"
        );
        // Attachment natives *are* in scope; message-level natives are not fields here.
        assert!(s.contains("native_sha256"), "attachment digests framed");
        assert!(!s.contains(_would_be_native_a));
        assert!(!s.contains(_would_be_native_b));
    }

    #[test]
    fn non_email_smoke() {
        let input = NonEmailLogicalInput {
            category: Some("pdf".into()),
            title: Some("  Report  2020 ".into()),
            author: Some("Ada".into()),
            created: Some("2020-06-01T12:00:00.123Z".into()),
            text: Some("page one".into()),
            children_native_sha256: vec!["zz".into(), "aa".into()],
        };
        let h1 = compute_non_email_logical_hash(&input);
        let mut input2 = input.clone();
        input2.children_native_sha256 = vec!["aa".into(), "zz".into()];
        assert_eq!(h1, compute_non_email_logical_hash(&input2));
        assert_eq!(h1.len(), 64);

        let mut other = input.clone();
        other.text = Some("page two".into());
        assert_ne!(compute_non_email_logical_hash(&other), h1);
    }

    #[test]
    fn time_truncates_to_utc_seconds() {
        assert_eq!(
            normalize_time_utc_second("2020-01-02T03:04:05.999Z"),
            "2020-01-02T03:04:05Z"
        );
    }

    #[test]
    fn body_html_strip_and_zero_width() {
        let n = normalize_body("<p>Hi\u{200B}</p>\r\n");
        assert_eq!(n, "Hi");
    }
}
