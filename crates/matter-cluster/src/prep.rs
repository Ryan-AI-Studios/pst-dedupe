//! eDiscovery text prep: header-line strip + confidentiality boilerplate (versioned).

/// Prep list version token (fingerprint lineage).
pub const PREP_VERSION: &str = "ediscovery_prep_v1";

/// Case-insensitive line prefixes dropped entirely (header lines).
const HEADER_LINE_PREFIXES: &[&str] = &[
    "from:",
    "to:",
    "cc:",
    "bcc:",
    "sent:",
    "date:",
    "subject:",
    "reply-to:",
    "importance:",
    "attachments:",
];

/// Whole-line markers for forwarded / original message blocks.
const HEADER_LINE_MARKERS: &[&str] = &[
    "-----original message-----",
    "----- original message -----",
    "begin forwarded message",
    "---------- forwarded message ----------",
];

/// Confidentiality / disclaimer phrase fragments (case-insensitive substring).
///
/// Vocab hygiene only — **not** privilege detection.
const DISCLAIMER_PHRASES: &[&str] = &[
    "privileged and confidential",
    "attorney-client privileged",
    "attorney client privileged",
    "intended recipient",
    "intended solely for",
    "unauthorized disclosure",
    "confidentiality notice",
    "if you are not the intended",
    "please delete this email",
    "this email and any attachments",
    "may contain confidential",
    "strictly confidential",
    "do not distribute",
];

/// Structural whole-token labels dropped after tokenize.
const STRUCTURAL_TOKENS: &[&str] = &["from", "sent", "subject", "mailto", "cc", "bcc"];

/// Strip email header lines and confidentiality boilerplate lines.
pub fn strip_headers_and_disclaimers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if HEADER_LINE_PREFIXES.iter().any(|p| lower.starts_with(p)) {
            continue;
        }
        if HEADER_LINE_MARKERS.iter().any(|m| lower.contains(m)) {
            continue;
        }
        if DISCLAIMER_PHRASES.iter().any(|p| lower.contains(p)) {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(trimmed);
    }
    out
}

/// True if token is a structural mail label (whole token).
pub fn is_structural_token(token: &str) -> bool {
    STRUCTURAL_TOKENS.contains(&token)
}

/// Prep version for fingerprint.
pub fn prep_fingerprint_token() -> &'static str {
    PREP_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_header_lines_keeps_body() {
        let raw =
            "From: a@example.com\nTo: b@example.com\nSubject: hi\n\nInvoice payment overdue vendor.\n";
        let cleaned = strip_headers_and_disclaimers(raw);
        assert!(!cleaned.to_lowercase().contains("from:"));
        assert!(cleaned.to_lowercase().contains("invoice"));
    }

    #[test]
    fn strips_disclaimer() {
        let raw = "Deal terms for merger.\nThis message is privileged and confidential and intended solely for the recipient.\nMore deal terms.";
        let cleaned = strip_headers_and_disclaimers(raw);
        assert!(!cleaned.to_lowercase().contains("privileged"));
        assert!(cleaned.contains("Deal terms"));
        assert!(cleaned.contains("More deal"));
    }
}
