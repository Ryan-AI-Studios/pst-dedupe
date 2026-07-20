//! Footer / disclaimer strip for sentiment scoring (hygiene only — not privilege).
//!
//! Phrase list aligned with `matter-cluster` prep for shared eDiscovery hygiene.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_disclaimer() {
        let raw = "Hostile message here.\nThis message is privileged and confidential and intended solely for the recipient.\n";
        let cleaned = strip_headers_and_disclaimers(raw);
        assert!(!cleaned.to_lowercase().contains("privileged"));
        assert!(cleaned.contains("Hostile"));
    }
}
