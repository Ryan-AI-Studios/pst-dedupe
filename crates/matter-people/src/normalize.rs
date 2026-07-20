//! Participant normalization (smtp / display / x500 / other).

use matter_core::identity_kind;
use matter_entity::normalize_email;

/// Normalized participant identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedParticipant {
    pub identity_kind: String,
    pub normalized_key: String,
    /// Populated only for `smtp`.
    pub email_domain: Option<String>,
    /// Best-seen raw display label (trimmed original).
    pub display_label: String,
}

/// Normalize a raw participant string.
///
/// Returns `None` only when empty after normalize (skip).
///
/// Rules (LOCKED):
/// - **SMTP** if looks like email (`local@domain`, domain need **not** contain `.`):
///   prefer [`matter_entity::normalize_email`] (trim, strip edge punctuation, case-fold).
///   For single-label domains (e.g. `alice@corp`) that `normalize_email` rejects, apply the
///   same punctuation strip + ASCII case-fold and keep `identity_kind = smtp`.
/// - **X.500**: starts with `/o=` or `/O=`, or contains `/cn=` / `/CN=`, or
///   classic `EX:/o=` form → trim + Unicode case-fold whole string; domain None.
/// - **Display / other**: trim, collapse whitespace, Unicode case-fold; do not drop.
pub fn normalize_participant(raw: &str) -> Option<NormalizedParticipant> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Prefer SMTP when the string looks like an email (after edge punctuation strip).
    if looks_like_email(trimmed) {
        if let Some(norm) = normalize_email(trimmed) {
            let domain = norm.split_once('@').map(|(_, d)| d.to_string());
            return Some(NormalizedParticipant {
                identity_kind: identity_kind::SMTP.into(),
                normalized_key: norm,
                email_domain: domain,
                display_label: trimmed.to_string(),
            });
        }
        // Single-label / no-dot domains (e.g. alice@corp): still SMTP for people graph.
        if let Some(norm) = normalize_smtp_single_label(trimmed) {
            let domain = norm.split_once('@').map(|(_, d)| d.to_string());
            return Some(NormalizedParticipant {
                identity_kind: identity_kind::SMTP.into(),
                normalized_key: norm,
                email_domain: domain,
                display_label: trimmed.to_string(),
            });
        }
    }

    if is_x500(trimmed) {
        let key = trimmed.to_lowercase();
        if key.is_empty() {
            return None;
        }
        return Some(NormalizedParticipant {
            identity_kind: identity_kind::X500.into(),
            normalized_key: key,
            email_domain: None,
            display_label: trimmed.to_string(),
        });
    }

    // Display / other: collapse whitespace + Unicode case-fold.
    let collapsed = collapse_ws(trimmed);
    if collapsed.is_empty() {
        return None;
    }
    let key = collapsed.to_lowercase();
    // Prefer `display` for human-looking strings; `other` only when no letters.
    let kind = if key.chars().any(|c| c.is_alphabetic()) {
        identity_kind::DISPLAY
    } else {
        identity_kind::OTHER
    };
    Some(NormalizedParticipant {
        identity_kind: kind.into(),
        normalized_key: key,
        email_domain: None,
        display_label: collapsed,
    })
}

/// Strip edge punctuation often stuck to address captures (mirrors 0046 helper).
fn strip_edges(s: &str) -> &str {
    const EDGE: &[char] = &[
        ',', '.', ';', ':', ')', '(', '<', '>', '"', '\'', '[', ']', '{', '}', '!', '?',
    ];
    s.trim_matches(EDGE)
}

/// `local@domain` with non-empty sides; domain **may** lack a dot (`alice@corp`).
fn looks_like_email(s: &str) -> bool {
    let t = strip_edges(s.trim());
    if let Some((local, domain)) = t.split_once('@') {
        !local.is_empty()
            && !domain.is_empty()
            && !local.contains('@')
            && !domain.contains([' ', '\t', '\n', '\r'])
            && !local.contains([' ', '\t', '\n', '\r'])
    } else {
        false
    }
}

/// SMTP normalize for addresses `normalize_email` rejects (typically no `.` in domain).
fn normalize_smtp_single_label(raw: &str) -> Option<String> {
    let s = strip_edges(raw.trim());
    if s.is_empty() {
        return None;
    }
    let (local, domain) = s.split_once('@')?;
    if local.is_empty() || domain.is_empty() || domain.contains('.') {
        // Dotted domains belong to normalize_email; if it failed, do not invent.
        return None;
    }
    if local.contains([' ', '\t']) || domain.contains([' ', '\t']) {
        return None;
    }
    Some(format!(
        "{}@{}",
        local.to_ascii_lowercase(),
        domain.to_ascii_lowercase()
    ))
}

fn is_x500(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("/o=") || lower.starts_with("ex:/o=") {
        return true;
    }
    lower.contains("/cn=") || lower.contains("/o=")
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_john_doe_not_dropped() {
        let p = normalize_participant("John Doe").expect("display");
        assert_eq!(p.identity_kind, identity_kind::DISPLAY);
        assert_eq!(p.normalized_key, "john doe");
        assert!(p.email_domain.is_none());
    }

    #[test]
    fn x500_dn() {
        let raw = "/o=Exch/ou=Exchange Administrative Group/cn=Recipients/cn=jdoe";
        let p = normalize_participant(raw).expect("x500");
        assert_eq!(p.identity_kind, identity_kind::X500);
        assert_eq!(p.normalized_key, raw.to_lowercase());
        assert!(p.email_domain.is_none());
    }

    #[test]
    fn smtp_trailing_comma() {
        let a = normalize_participant("bob@example.com,").expect("smtp");
        let b = normalize_participant("bob@example.com").expect("smtp");
        assert_eq!(a.identity_kind, identity_kind::SMTP);
        assert_eq!(a.normalized_key, b.normalized_key);
        assert_eq!(a.email_domain.as_deref(), Some("example.com"));
    }

    #[test]
    fn smtp_single_label_domain_no_dot() {
        let p = normalize_participant("alice@corp").expect("smtp");
        assert_eq!(p.identity_kind, identity_kind::SMTP);
        assert_eq!(p.normalized_key, "alice@corp");
        assert_eq!(p.email_domain.as_deref(), Some("corp"));

        let upper = normalize_participant("Alice@CORP").expect("smtp");
        assert_eq!(upper.normalized_key, "alice@corp");
        assert_eq!(upper.email_domain.as_deref(), Some("corp"));
    }

    #[test]
    fn display_unicode_case_fold() {
        // Simple non-ASCII letter pair: É → é via Unicode lowercase.
        let p = normalize_participant("Élodie Dupont").expect("display");
        assert_eq!(p.identity_kind, identity_kind::DISPLAY);
        assert_eq!(p.normalized_key, "élodie dupont");
    }

    #[test]
    fn empty_skipped() {
        assert!(normalize_participant("   ").is_none());
        assert!(normalize_participant("").is_none());
    }

    #[test]
    fn ex_prefix_x500() {
        let p = normalize_participant("EX:/o=Contoso/ou=Exchange/cn=Recipients/cn=alice")
            .expect("ex x500");
        assert_eq!(p.identity_kind, identity_kind::X500);
    }
}
